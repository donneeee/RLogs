#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 34;
const SUPPORTED_SCHEMA_VERSIONS = new Set([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34]);
const GENERATOR = "tools/bpsr-blade-sweep-scalar-proof.mjs";
const DIRECT_SKILL_ID = 3914;
const COMPOUND_SKILL_ID = 3946;
const DIRECT_EFFECT_ID = 391401;
const COMPOUND_EFFECT_ID = 394601;
const PROVIDER_ITEM_ID = 3000045;
const COMPONENT_ID = "goblin-march-shared-blade-sweep-target-armor-reduction";
const EXPECTED_LEVELS = [1, 2, 3, 4, 5];
const EXPECTED_BLOCK_BASIS_POINTS = [150, 300, 450, 600, 750];
const EXPECTED_ARMOR_BASIS_POINTS = [130, 260, 390, 520, 650];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "analyze") analyzeCommand(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyzeCommand(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const effectId = positiveInteger(required(parsed, "effect"), "effect");
  const inputPaths = {
    buildSourceManifest: resolved(parsed, "build-source-manifest"),
    skillTable: resolved(parsed, "skill-table"),
    skillEffectTable: resolved(parsed, "skill-effect-table"),
    aoyiStarTable: resolved(parsed, "aoyi-star-table"),
    buffTable: resolved(parsed, "buff-table"),
    runtimeProof: resolved(parsed, "runtime-proof"),
    providerOwnershipProof: resolved(parsed, "provider-ownership-proof"),
    providerOwnershipGapWorklist: resolved(parsed, "provider-ownership-gap-worklist"),
    counterfactualRollup: resolved(parsed, "counterfactual-rollup"),
    targetMitigationRollup: resolved(parsed, "target-mitigation-rollup"),
    globalTargetMitigationRollup: resolved(parsed, "global-target-mitigation-rollup"),
    targetMitigationOfflineExhaustion: resolved(parsed, "target-mitigation-offline-exhaustion"),
    targetMitigationAcquisitionWorklist: resolved(parsed, "target-mitigation-acquisition-worklist"),
    targetMitigationNearPairCandidateProof: resolved(parsed, "target-mitigation-near-pair-candidate-proof"),
    counterfactualDiscriminants: resolved(parsed, "counterfactual-discriminants"),
    fightAttributeTransformSurface: resolved(parsed, "fight-attribute-transform-surface"),
    fightAttributeTransformEvaluatorProof: resolved(
      parsed,
      "fight-attribute-transform-evaluator-proof",
    ),
    masteryRuntimeRouteProof: resolved(parsed, "mastery-runtime-route-proof"),
    targetStatusActionRouteAudit: resolved(parsed, "target-status-action-route-audit"),
    targetDefensePercentLifecycleProof: resolved(parsed, "target-defense-percent-lifecycle-proof"),
    targetDefenseFightAttributeScopeProof: resolved(parsed, "target-defense-fight-attribute-scope-proof"),
    targetDefenseStatusDiagnosticRollup: resolved(parsed, "target-defense-status-diagnostic-rollup"),
    targetMitigationActorSceneExhaustion: resolved(parsed, "target-mitigation-actor-scene-exhaustion"),
    rlogGapWindowAudit: resolved(parsed, "rlog-gap-window-audit"),
    targetEffectFormulaProof: resolved(parsed, "target-effect-formula-proof"),
    rlogTransitionCounterfactualAudit: resolved(parsed, "rlog-transition-counterfactual-audit"),
    rlogOpaqueAttributeAudit: resolved(parsed, "rlog-opaque-attribute-audit"),
    sourceStatusConfounderRouteAudit: resolved(parsed, "source-status-confounder-route-audit"),
    sourceStatusLocalObservableAudit: resolved(parsed, "source-status-local-observable-audit"),
    luckyPacketComponentProof: resolved(parsed, "lucky-packet-component-proof"),
    mattackLuckyMitigationDiagnostic: resolved(parsed, "mattack-lucky-mitigation-diagnostic"),
    attackLuckyMitigationDiagnostic: resolved(parsed, "attack-lucky-mitigation-diagnostic"),
    luckyParentMultiplierProof: resolved(parsed, "lucky-parent-multiplier-proof"),
  };
  const output = path.resolve(required(parsed, "output"));
  const inputs = Object.fromEntries(Object.entries(inputPaths).map(([key, file]) => [
    camelToSnake(key),
    fileDescriptor(file),
  ]));
  const report = buildReport({
    build,
    effectId,
    buildSourceManifest: readJson(inputPaths.buildSourceManifest, "complete build source manifest"),
    skillTable: readJson(inputPaths.skillTable, "SkillTable"),
    skillEffectTable: readJson(inputPaths.skillEffectTable, "SkillEffectTable"),
    aoyiStarTable: readJson(inputPaths.aoyiStarTable, "SkillAoyiStarTable"),
    buffTable: readJson(inputPaths.buffTable, "BuffTable"),
    runtimeProof: readJson(inputPaths.runtimeProof, "runtime provider/recipient proof"),
    providerOwnershipProof: readJson(inputPaths.providerOwnershipProof, "provider ownership proof"),
    providerOwnershipGapWorklist: readJson(
      inputPaths.providerOwnershipGapWorklist,
      "provider ownership gap worklist",
    ),
    counterfactualRollup: readJson(inputPaths.counterfactualRollup, "counterfactual rollup"),
    targetMitigationRollup: readJson(inputPaths.targetMitigationRollup, "target mitigation rollup"),
    globalTargetMitigationRollup: readJson(
      inputPaths.globalTargetMitigationRollup,
      "global target mitigation rollup",
    ),
    targetMitigationOfflineExhaustion: readJson(
      inputPaths.targetMitigationOfflineExhaustion,
      "target mitigation offline exhaustion proof",
    ),
    targetMitigationAcquisitionWorklist: readJson(
      inputPaths.targetMitigationAcquisitionWorklist,
      "target mitigation acquisition worklist",
    ),
    targetMitigationNearPairCandidateProof: readJson(
      inputPaths.targetMitigationNearPairCandidateProof,
      "target mitigation near-pair candidate proof",
    ),
    counterfactualDiscriminants: readJson(
      inputPaths.counterfactualDiscriminants,
      "Blade Sweep counterfactual discriminants",
    ),
    fightAttributeTransformSurface: readJson(
      inputPaths.fightAttributeTransformSurface,
      "FightAttrTranTable surface",
    ),
    fightAttributeTransformEvaluatorProof: readJson(
      inputPaths.fightAttributeTransformEvaluatorProof,
      "fight-attribute transform evaluator proof",
    ),
    masteryRuntimeRouteProof: readJson(
      inputPaths.masteryRuntimeRouteProof,
      "current-season runtime route proof",
    ),
    targetStatusActionRouteAudit: readJson(
      inputPaths.targetStatusActionRouteAudit,
      "target-status action-route audit",
    ),
    targetDefensePercentLifecycleProof: readJson(
      inputPaths.targetDefensePercentLifecycleProof,
      "target defense percent lifecycle proof",
    ),
    targetDefenseFightAttributeScopeProof: readJson(
      inputPaths.targetDefenseFightAttributeScopeProof,
      "target defense fight-attribute scope proof",
    ),
    targetDefenseStatusDiagnosticRollup: readJson(
      inputPaths.targetDefenseStatusDiagnosticRollup,
      "target defense status diagnostic rollup",
    ),
    targetMitigationActorSceneExhaustion: readJson(
      inputPaths.targetMitigationActorSceneExhaustion,
      "target mitigation actor-scene exhaustion",
    ),
    rlogGapWindowAudit: readJson(
      inputPaths.rlogGapWindowAudit,
      "effect RLOG gap-window audit",
    ),
    targetEffectFormulaProof: readJson(
      inputPaths.targetEffectFormulaProof,
      "gap-bounded target-effect formula proof",
    ),
    rlogTransitionCounterfactualAudit: readJson(
      inputPaths.rlogTransitionCounterfactualAudit,
      "effect RLOG transition counterfactual audit",
    ),
    rlogOpaqueAttributeAudit: readJson(
      inputPaths.rlogOpaqueAttributeAudit,
      "RLOG opaque attribute audit",
    ),
    sourceStatusConfounderRouteAudit: readJson(
      inputPaths.sourceStatusConfounderRouteAudit,
      "source-status confounder route audit",
    ),
    sourceStatusLocalObservableAudit: readJson(
      inputPaths.sourceStatusLocalObservableAudit,
      "source-status local-observable audit",
    ),
    luckyPacketComponentProof: readJson(
      inputPaths.luckyPacketComponentProof,
      "same-build Lucky packet component proof",
    ),
    mattackLuckyMitigationDiagnostic: readJson(
      inputPaths.mattackLuckyMitigationDiagnostic,
      "same-build MAttackLucky mitigation diagnostic",
    ),
    attackLuckyMitigationDiagnostic: readJson(
      inputPaths.attackLuckyMitigationDiagnostic,
      "same-build AttackLucky mitigation diagnostic",
    ),
    luckyParentMultiplierProof: readJson(
      inputPaths.luckyParentMultiplierProof,
      "same-build Lucky parent and multiplier proof",
    ),
    inputs,
  });
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(`wrote ${output}`);
}

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  verifyReport(readJson(input, "Blade Sweep scalar proof"));
  console.log(`verified ${input}`);
}

function buildReport(context) {
  if (context.effectId !== 2110092) {
    throw new Error("This focused proof only accepts exact effect 2110092");
  }
  const skillRows = tableRows(context.skillTable, "SkillTable");
  const effectRows = tableRows(context.skillEffectTable, "SkillEffectTable");
  const starRows = tableRows(context.aoyiStarTable, "SkillAoyiStarTable");
  const buffRows = tableRows(context.buffTable, "BuffTable");
  const buildIdentity = validateBuildSourceManifest(
    context.buildSourceManifest,
    context.build,
    context.inputs,
  );
  const directSkill = exactRow(skillRows, "Id", DIRECT_SKILL_ID, "direct skill");
  const compoundSkill = exactRow(skillRows, "Id", COMPOUND_SKILL_ID, "compound skill");
  const directEffect = exactRow(effectRows, "Id", DIRECT_EFFECT_ID, "direct skill effect");
  const compoundEffect = exactRow(effectRows, "Id", COMPOUND_EFFECT_ID, "compound skill effect");
  const buff = exactRow(buffRows, "Id", context.effectId, "target status");

  assertExactIntegers(directSkill.EffectIDs, [DIRECT_EFFECT_ID], "direct SkillTable EffectIDs");
  assertExactIntegers(compoundSkill.EffectIDs, [COMPOUND_EFFECT_ID], "compound SkillTable EffectIDs");
  if (Number(directEffect.SkillId) !== DIRECT_SKILL_ID || Number(compoundEffect.SkillId) !== COMPOUND_SKILL_ID) {
    throw new Error("SkillEffectTable skill links do not match the exact skill IDs");
  }
  const directLabels = semanticLabels(directEffect);
  const compoundLabels = semanticLabels(compoundEffect);
  requireLabels(directLabels, ["Block DMG Reduction Bonus", "Armor Penetration"], "direct effect");
  requireLabels(compoundLabels, [
    '"Blade Sweep" Block DMG Reduction Bonus',
    '"Blade Sweep" Armor Penetration',
  ], "compound effect");
  assertExactIntegers(buff.RepeatAddRule, [1, 3], "BuffTable RepeatAddRule");
  const durations = (buff.DestroyParam ?? []).flat().map(Number);
  if (!durations.includes(10)) throw new Error("BuffTable exact effect does not preserve a 10 second lifecycle");

  const directLadder = scalarLadder(starRows, DIRECT_SKILL_ID, "attrPer", "attrAdd");
  const compoundLadder = scalarLadder(starRows, COMPOUND_SKILL_ID, "attrPer3914", "attrAdd3914");
  assertLadder(directLadder, EXPECTED_BLOCK_BASIS_POINTS, EXPECTED_ARMOR_BASIS_POINTS, "direct");
  assertLadder(compoundLadder, EXPECTED_BLOCK_BASIS_POINTS, EXPECTED_ARMOR_BASIS_POINTS, "compound");
  if (JSON.stringify(directLadder) !== JSON.stringify(compoundLadder)) {
    throw new Error("Goblin March Blade Sweep aliases do not exactly match the direct Blade Sweep ladder");
  }

  const runtime = validateRuntimeProof(context.runtimeProof, context.build, context.effectId);
  const ownership = validateOwnershipProof(context.providerOwnershipProof, context.build, context.effectId);
  const ownershipGapWorklist = validateOwnershipGapWorklist(
    context.providerOwnershipGapWorklist,
    context.build,
    context.effectId,
    context.inputs.provider_ownership_proof,
    ownership,
  );
  const counterfactual = validateCounterfactualRollup(context.counterfactualRollup, context.build, context.effectId);
  const targetMitigation = validateTargetMitigationRollup(
    context.targetMitigationRollup,
    context.build,
    counterfactual,
  );
  const globalTargetMitigation = validateGlobalTargetMitigationRollup(
    context.globalTargetMitigationRollup,
    context.build,
    targetMitigation,
  );
  const targetMitigationOfflineExhaustion = validateTargetMitigationOfflineExhaustion(
    context.targetMitigationOfflineExhaustion,
    context.build,
    context.inputs.global_target_mitigation_rollup,
    globalTargetMitigation,
  );
  const targetMitigationAcquisitionWorklist = validateTargetMitigationAcquisitionWorklist(
    context.targetMitigationAcquisitionWorklist,
    context.build,
    context.effectId,
    targetMitigation,
  );
  const targetMitigationNearPairCandidate = validateTargetMitigationNearPairCandidateProof(
    context.targetMitigationNearPairCandidateProof,
    context.build,
  );
  const targetDefenseTransformBoundary = validateTargetDefenseTransformBoundary(
    context.fightAttributeTransformSurface,
    context.fightAttributeTransformEvaluatorProof,
    context.masteryRuntimeRouteProof,
    context.build,
    context.inputs.fight_attribute_transform_evaluator_proof,
  );
  const counterfactualDiscriminants = validateCounterfactualDiscriminants(
    context.counterfactualDiscriminants,
    context.build,
    context.effectId,
  );
  const targetStatusActionRouteAudit = validateTargetStatusActionRouteAudit(
    context.targetStatusActionRouteAudit,
    context.build,
  );
  const targetDefensePercentLifecycleProof = validateTargetDefensePercentLifecycleProof(
    context.targetDefensePercentLifecycleProof,
    context.build,
  );
  const targetDefenseFightAttributeScopeProof = validateTargetDefenseFightAttributeScopeProof(
    context.targetDefenseFightAttributeScopeProof,
    context.build,
  );
  const targetDefenseStatusDiagnosticRollup = validateTargetDefenseStatusDiagnosticRollup(
    context.targetDefenseStatusDiagnosticRollup,
    context.build,
  );
  const targetMitigationActorSceneExhaustion = validateTargetMitigationActorSceneExhaustion(
    context.targetMitigationActorSceneExhaustion,
    context.build,
  );
  const rlogGapWindowAudit = validateRlogGapWindowAudit(
    context.rlogGapWindowAudit,
    context.build,
    context.effectId,
    context.globalTargetMitigationRollup,
    context.inputs.global_target_mitigation_rollup,
  );
  const targetEffectFormulaProof = validateTargetEffectFormulaProof(
    context.targetEffectFormulaProof,
    context.build,
    context.effectId,
    rlogGapWindowAudit,
    context.inputs.rlog_gap_window_audit,
  );
  const rlogTransitionCounterfactualAudit = validateRlogTransitionCounterfactualAudit(
    context.rlogTransitionCounterfactualAudit,
    context.build,
    context.effectId,
    context.inputs.rlog_gap_window_audit,
  );
  const rlogOpaqueAttributeAudit = validateRlogOpaqueAttributeAudit(
    context.rlogOpaqueAttributeAudit,
    context.build,
    context.effectId,
    context.inputs.rlog_gap_window_audit,
  );
  const sourceStatusConfounderRouteAudit = validateSourceStatusConfounderRouteAudit(
    context.sourceStatusConfounderRouteAudit,
    context.build,
  );
  const sourceStatusLocalObservableAudit = validateSourceStatusLocalObservableAudit(
    context.sourceStatusLocalObservableAudit,
    context.build,
  );
  const luckyPacketComponentProof = validateLuckyPacketComponentProof(
    context.luckyPacketComponentProof,
    context.build,
  );
  const mattackLuckyMitigationDiagnostic = validateMAttackLuckyMitigationDiagnostic(
    context.mattackLuckyMitigationDiagnostic,
    context.build,
    luckyPacketComponentProof,
  );
  const attackLuckyMitigationDiagnostic = validateAttackLuckyMitigationDiagnostic(
    context.attackLuckyMitigationDiagnostic,
    context.build,
    luckyPacketComponentProof,
  );
  const luckyParentMultiplierProof = validateLuckyParentMultiplierProof(
    context.luckyParentMultiplierProof,
    context.build,
  );
  assertSameRlogSet(ownership.rlogs, counterfactual.rlogs);
  const observedTiers = [...new Set(runtime.observations.map((entry) => Number(entry.equipped_tier)))].sort();
  const observedTierScalars = observedTiers.map((tier) => {
    const row = compoundLadder.find((entry) => entry.level === tier);
    if (!row) throw new Error(`Runtime tier ${tier} has no exact static ladder row`);
    return {
      tier,
      armor_penetration_raw_basis_points: row.armor_penetration_raw_basis_points,
      armor_penetration_percent: row.armor_penetration_percent,
    };
  });

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: context.build,
    effect_id: context.effectId,
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      exact_input_hashes_are_embedded: true,
      localized_names_and_semantic_labels_are_evidence_only: true,
      exact_static_scalar_does_not_prove_armor_to_damage_equation: true,
      provider_ownership_must_be_packet_proven_for_every_lifecycle_event: true,
      unresolved_provider_events_are_preserved: true,
      packet_absence_is_not_zero: true,
      aggregate_offline_exhaustion_is_not_combat_formula_proof: true,
      target_status_relaxed_near_pairs_are_not_combat_formula_proof: true,
      status_confounded_integer_candidate_compatibility_is_not_combat_formula_proof: true,
      structurally_unobservable_remote_player_packets_are_not_formula_acquisition_requirements: true,
      candidate_counterfactual_discriminants_never_grant_formula_or_ui_authority: true,
      exact_character_sheet_transform_does_not_prove_combat_stage_binding: true,
      exact_packet_component_and_coefficient_identity_do_not_prove_defense_stage_order: true,
      same_input_status_invariance_does_not_remove_common_target_status_confounders: true,
      produced_action_routes_do_not_prove_status_modifier_damage_neutrality: true,
      exact_defense_stat_formula_does_not_prove_target_defense_to_damage_projection: true,
      defense_final_only_observations_are_preserved_without_claiming_raw_percent_packet_visibility: true,
      complete_observed_fight_attribute_scope_does_not_exclude_hidden_damage_logic: true,
      sparse_crit_co_updates_do_not_establish_an_unconditional_secondary_component: true,
      exhaustive_local_status_diagnostics_do_not_make_confounded_near_pairs_authoritative: true,
      actor_scene_cross_capture_exhaustion_does_not_make_actor_shape_formula_authoritative: true,
      complete_gap_bounded_lifecycle_windows_do_not_make_counterfactual_formula_authority: true,
      gap_bounded_formula_rows_without_target_defense_do_not_prove_the_mitigation_curve: true,
      transition_adjacent_candidate_search_never_grants_counterfactual_formula_authority: true,
      attribute_443_474_and_target_current_hp_exclusion_is_diagnostic_only: true,
      lucky_packet_component_identity_does_not_prove_defense_dependency_or_formula_semantics: true,
      absent_observed_mitigation_axes_are_not_zero_mitigation_or_formula_proof: true,
      both_lucky_families_require_observed_mitigation_inputs_before_route_selection: true,
      complete_observed_lucky_parent_binding_does_not_invent_multiplier_formula_semantics: true,
      complete_local_source_attribute_candidate_exhaustion_does_not_invent_multi_input_formula_semantics: true,
      opaque_attribute_wire_shape_and_timing_never_grant_semantic_exclusion: true,
      healing_only_source_action_does_not_grant_status_confounder_exclusion: true,
      locally_observed_dynamic_recipient_deltas_do_not_invent_remote_provider_inputs: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: structuredClone(context.inputs),
    build_identity: buildIdentity,
    exact_identity: {
      direct_skill: { skill_id: DIRECT_SKILL_ID, skill_effect_id: DIRECT_EFFECT_ID },
      compound_skill: {
        skill_id: COMPOUND_SKILL_ID,
        skill_effect_id: COMPOUND_EFFECT_ID,
        provider_item_id: PROVIDER_ITEM_ID,
      },
      target_status: {
        effect_id: context.effectId,
        duration_seconds: 10,
        repeat_add_rule: [1, 3],
      },
      runtime_component_id: COMPONENT_ID,
    },
    semantic_evidence: {
      direct_skill_name: String(directSkill.Name ?? ""),
      compound_skill_name: String(compoundSkill.Name ?? ""),
      direct_skill_effect_labels: directLabels,
      compound_skill_effect_labels: compoundLabels,
    },
    static_scalar: {
      unit_interpretation: "raw integer basis points; 100 basis points = 1 percent",
      direct_blade_sweep_ladder: directLadder,
      goblin_march_blade_sweep_alias_ladder: compoundLadder,
      ladders_exactly_equal: true,
      exact_static_scalar_proven: true,
    },
    runtime_binding: {
      component_id: COMPONENT_ID,
      observation_rows: runtime.observations.length,
      grouped_status_rows: Number(runtime.component.summary.status_rows),
      provider_actor_ids: uniqueSortedNumbers(runtime.observations, "provider_actor_id"),
      provider_entity_uuids: uniqueSortedStrings(runtime.observations, "provider_entity_uuid"),
      provider_item_ids: uniqueSortedNumbers(runtime.observations, "equipped_item_id"),
      observed_tiers: observedTiers,
      target_kinds: [...new Set(runtime.observations.map((entry) => String(entry.target_kind)))].sort(),
      observed_tier_scalars: observedTierScalars,
      exact_effect_item_tier_target_binding_observed: true,
    },
    provider_ownership: {
      input_rlogs: ownership.rlogs,
      selected_status_events: ownership.selected,
      events_with_stable_player_character_id: ownership.stable,
      events_with_prior_status_instance_player_owner: ownership.priorStatusInstance,
      events_with_same_wire_packet_player_owner: ownership.sameWirePacket,
      unresolved_status_events: ownership.unresolved,
      exact_provider_ownership_for_every_event_proven: ownership.unresolved === 0,
      unresolved_events_preserved: true,
    },
    provider_ownership_gap_worklist: ownershipGapWorklist,
    counterfactual_projection: {
      input_rlogs: counterfactual.rlogs,
      matching_capture_runs: counterfactual.matchingRuns,
      formula_damage_samples: counterfactual.formulaSamples,
      source_locus_observed_samples: counterfactual.sourceSamples,
      target_locus_observed_samples: counterfactual.targetSamples,
      exact_controlled_groups: counterfactual.exactControlledGroups,
      exact_sample_comparisons: counterfactual.exactSampleComparisons,
      exact_damage_projection_proven: false,
      exact_armor_to_damage_equation_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
    },
    target_mitigation_evidence: targetMitigation,
    global_target_mitigation_evidence: globalTargetMitigation,
    target_mitigation_offline_exhaustion: targetMitigationOfflineExhaustion,
    target_mitigation_acquisition_worklist: targetMitigationAcquisitionWorklist,
    target_mitigation_near_pair_candidate: targetMitigationNearPairCandidate,
    target_defense_transform_boundary: targetDefenseTransformBoundary,
    counterfactual_discriminants: counterfactualDiscriminants,
    target_status_action_route_audit: targetStatusActionRouteAudit,
    target_defense_percent_lifecycle_proof: targetDefensePercentLifecycleProof,
    target_defense_fight_attribute_scope_proof: targetDefenseFightAttributeScopeProof,
    target_defense_status_diagnostic_rollup: targetDefenseStatusDiagnosticRollup,
    target_mitigation_actor_scene_exhaustion: targetMitigationActorSceneExhaustion,
    rlog_gap_window_audit: rlogGapWindowAudit,
    target_effect_formula_proof: targetEffectFormulaProof,
    rlog_transition_counterfactual_audit: rlogTransitionCounterfactualAudit,
    rlog_opaque_attribute_audit: rlogOpaqueAttributeAudit,
    source_status_confounder_route_audit: sourceStatusConfounderRouteAudit,
    source_status_local_observable_audit: sourceStatusLocalObservableAudit,
    lucky_packet_component_proof: luckyPacketComponentProof,
    mattack_lucky_mitigation_diagnostic: mattackLuckyMitigationDiagnostic,
    attack_lucky_mitigation_diagnostic: attackLuckyMitigationDiagnostic,
    lucky_parent_multiplier_proof: luckyParentMultiplierProof,
    summary: {
      exact_static_scalar_proven: true,
      observed_runtime_tier: observedTiers.length === 1 ? observedTiers[0] : null,
      observed_runtime_armor_penetration_basis_points:
        observedTierScalars.length === 1 ? observedTierScalars[0].armor_penetration_raw_basis_points : null,
      observed_runtime_armor_penetration_percent:
        observedTierScalars.length === 1 ? observedTierScalars[0].armor_penetration_percent : null,
      unresolved_provider_status_events: ownership.unresolved,
      target_mitigation_damage_samples: targetMitigation.damage_samples,
      target_mitigation_audited_axis_samples: targetMitigation.audited_axis_samples,
      target_mitigation_controlled_groups: targetMitigation.controlled_groups,
      maximum_target_mitigation_peak_working_set_mib:
        targetMitigation.maximum_measured_peak_working_set_mib,
      global_target_mitigation_damage_samples: globalTargetMitigation.damage_samples,
      global_target_mitigation_audited_axis_samples: globalTargetMitigation.audited_axis_samples,
      global_target_mitigation_controlled_groups: globalTargetMitigation.controlled_groups,
      offline_exhaustion_packet_capture_proofs:
        targetMitigationOfflineExhaustion.packet_capture_proofs,
      offline_exhaustion_promoted_combat_formulas:
        targetMitigationOfflineExhaustion.promoted_combat_formulas,
      target_status_relaxed_distinct_axis_pairs:
        targetMitigationAcquisitionWorklist.target_status_relaxed_distinct_axis_pairs,
      transformed_curve_status_confounded_compatible_rows:
        targetMitigationNearPairCandidate.transformed_curve_compatible_rows,
      runtime_simple_curve_status_confounded_compatible_rows:
        targetMitigationNearPairCandidate.runtime_simple_curve_compatible_rows,
      same_axis_target_status_pairs:
        targetMitigationNearPairCandidate.same_axis_status_invariance.physical_defense_same_axis_status_pairs,
      same_axis_equal_output_pairs:
        targetMitigationNearPairCandidate.same_axis_status_invariance.physical_defense_same_axis_equal_output_pairs,
      same_axis_divergent_output_pairs:
        targetMitigationNearPairCandidate.same_axis_status_invariance.physical_defense_same_axis_divergent_output_pairs,
      candidate_counterfactual_discriminant_rows:
        counterfactualDiscriminants.exact_discriminant_rows.length,
      candidate_counterfactual_distinct_output_signatures:
        counterfactualDiscriminants.distinct_predicted_damage_with_effect.length,
      current_season_id: targetDefenseTransformBoundary.current_season_id,
      exact_current_season_defense_curve_constant:
        targetDefenseTransformBoundary.exact_current_season_curve_constant,
      character_sheet_defense_transform_operation_order_proven:
        targetDefenseTransformBoundary.character_sheet_operation_order_proven,
      combat_defense_transform_stage_binding_proven:
        targetDefenseTransformBoundary.combat_stage_binding_proven,
      exact_packet_damage_component_id: counterfactualDiscriminants.packet_formula_identity.damage_attr_id,
      exact_packet_component_coefficient_basis_points:
        counterfactualDiscriminants.packet_formula_identity.pve_damage_ratio_basis_points[0],
      actor_scene_curve_compatible_rows:
        counterfactualDiscriminants.observed_baseline_curve.exact_curve_compatible_rows,
      actor_scene_curve_distinct_defense_points:
        counterfactualDiscriminants.observed_baseline_curve.selected_points.length,
      actor_scene_curve_status_confounded_rows:
        counterfactualDiscriminants.observed_baseline_curve.preserved_status_confounded_rows,
      actor_scene_compatible_target_status_states:
        counterfactualDiscriminants.observed_baseline_curve.same_input_status_invariance
          .compatible_target_status_state_ids,
      actor_scene_common_target_status_confounders:
        counterfactualDiscriminants.observed_baseline_curve.same_input_status_invariance
          .common_effect_ids_across_all_compatible_rows.length,
      actor_scene_varying_target_status_effects:
        counterfactualDiscriminants.observed_baseline_curve.same_input_status_invariance
          .varying_effect_ids_across_all_compatible_rows.length,
      actor_scene_isolated_invariant_single_effect_toggles:
        counterfactualDiscriminants.observed_baseline_curve.same_input_status_invariance
          .isolated_single_effect_toggle_count,
      target_status_action_route_audited_effects:
        targetStatusActionRouteAudit.audited_effects,
      target_status_confounders_eliminated_by_action_routes: 0,
      exact_effect_2201452_defense_stat_formula_proven: true,
      effect_2201452_exact_wire_transition_occurrences:
        targetDefensePercentLifecycleProof.exact_wire_occurrences,
      effect_2201452_packet_raw_percent_joined_occurrences:
        targetDefensePercentLifecycleProof.packet_raw_percent_joined_occurrences,
      effect_2201452_final_only_unresolved_occurrences:
        targetDefensePercentLifecycleProof.final_only_unresolved_occurrences,
      effect_2201452_selected_fight_attribute_components:
        targetDefenseFightAttributeScopeProof.selected_fight_attribute_components,
      effect_2201452_proven_reversible_constant_components:
        targetDefenseFightAttributeScopeProof.proven_reversible_constant_components,
      effect_2201452_unresolved_fight_attribute_components:
        targetDefenseFightAttributeScopeProof.unresolved_fight_attribute_components,
      effect_2201452_raw_armor_transitions_without_raw_crit_co_update:
        targetDefenseFightAttributeScopeProof.raw_armor_transitions_without_raw_crit_co_update,
      effect_2201452_exact_wire_independent_sessions:
        targetDefensePercentLifecycleProof.independent_sessions,
      effect_2201452_status_diagnostic_damage_samples:
        targetDefenseStatusDiagnosticRollup.damage_samples,
      effect_2201452_physical_defense_near_pairs:
        targetDefenseStatusDiagnosticRollup.physical_defense_unique_near_pairs,
      effect_2201452_near_pairs_with_effect_in_status_delta:
        targetDefenseStatusDiagnosticRollup.physical_defense_pairs_with_selected_effect_in_status_delta,
      effect_2201452_same_axis_damage_witnesses:
        targetDefenseStatusDiagnosticRollup.physical_defense_same_axis_pairs_with_selected_effect_in_status_delta,
      actor_scene_selected_ability_samples:
        targetMitigationActorSceneExhaustion.selected_ability_samples,
      actor_scene_physical_defense_samples:
        targetMitigationActorSceneExhaustion.physical_defense_samples,
      actor_scene_cross_capture_pairs:
        targetMitigationActorSceneExhaustion.cross_capture_actor_shape_pairs,
      actor_scene_stable_target_actor_ids:
        targetMitigationActorSceneExhaustion.physical_defense_samples_with_stable_target_actor_id,
      effect_2110092_gap_bounded_complete_lifecycles:
        rlogGapWindowAudit.complete_gap_bounded_lifecycle_count,
      effect_2110092_gap_bounded_windows_with_damage:
        rlogGapWindowAudit.complete_windows_with_damage_count,
      effect_2110092_gap_bounded_damage_events:
        rlogGapWindowAudit.damage_events_while_active,
      effect_2110092_lifecycles_cut_by_data_quality_boundaries:
        rlogGapWindowAudit.lifecycles_cut_by_data_quality_boundary,
      effect_2110092_gap_audited_damage_window_memberships:
        targetEffectFormulaProof.gap_audited_damage_window_memberships,
      effect_2110092_gap_matched_unique_damage_events:
        targetEffectFormulaProof.gap_matched_unique_damage_events,
      effect_2110092_gap_bounded_wire_start_formula_samples:
        targetEffectFormulaProof.formula_samples,
      effect_2110092_gap_bounded_rows_excluded_by_wire_start_status:
        targetEffectFormulaProof.gap_rows_excluded_by_wire_start_status,
      effect_2110092_gap_bounded_target_physical_defense_samples:
        targetEffectFormulaProof.target_physical_defense_samples,
      effect_2110092_transition_opposite_state_comparisons:
        rlogTransitionCounterfactualAudit.opposite_state_recent_comparisons,
      effect_2110092_transition_same_context_pairs:
        rlogTransitionCounterfactualAudit.same_normalized_damage_context_pairs,
      effect_2110092_transition_exact_input_pairs:
        rlogTransitionCounterfactualAudit.exact_observed_input_candidate_pairs,
      effect_2110092_transition_only_current_hp_difference_pairs:
        rlogTransitionCounterfactualAudit.same_context_pairs_with_only_target_current_hp_difference,
      effect_2110092_transition_pairs_after_443_474_exclusion:
        rlogTransitionCounterfactualAudit.same_context_pairs_after_443_474_attribute_exclusion,
      effect_2110092_transition_pairs_after_443_474_and_target_current_hp_exclusion:
        rlogTransitionCounterfactualAudit
          .same_context_pairs_after_443_474_and_target_current_hp_exclusion,
      effect_2110092_transition_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses:
        rlogTransitionCounterfactualAudit
          .same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses,
      effect_2110092_transition_minimum_residual_dimensions_after_diagnostic_exclusions:
        rlogTransitionCounterfactualAudit
          .minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion,
      closest_transition_lucky_damage_attr_id: luckyPacketComponentProof.selected_row.damage_attr_id,
      closest_transition_lucky_packet_damage_results:
        luckyPacketComponentProof.selected_row.packet_damage_results,
      lucky_packet_component_damage_results: luckyPacketComponentProof.packet_damage_results,
      lucky_packet_component_exact_matches: luckyPacketComponentProof.explicit_lucky_value_exact_matches,
      mattack_lucky_selected_damage_results: mattackLuckyMitigationDiagnostic.selected_sample_count,
      mattack_lucky_physical_defense_axis_samples:
        mattackLuckyMitigationDiagnostic.physical_defense_axis_samples,
      mattack_lucky_magic_defense_axis_samples:
        mattackLuckyMitigationDiagnostic.magic_defense_axis_samples,
      attack_lucky_selected_damage_results: attackLuckyMitigationDiagnostic.selected_sample_count,
      attack_lucky_physical_defense_axis_samples:
        attackLuckyMitigationDiagnostic.physical_defense_axis_samples,
      attack_lucky_magic_defense_axis_samples:
        attackLuckyMitigationDiagnostic.magic_defense_axis_samples,
      both_lucky_families_selected_damage_results:
        mattackLuckyMitigationDiagnostic.selected_sample_count +
        attackLuckyMitigationDiagnostic.selected_sample_count,
      lucky_parent_observed_events: luckyParentMultiplierProof.lucky_events,
      lucky_parent_unresolved_events: luckyParentMultiplierProof.unresolved_parent_events,
      lucky_multiplier_candidate_events: luckyParentMultiplierProof.multiplier_candidate_events,
      lucky_multiplier_candidate_exact_matches:
        luckyParentMultiplierProof.multiplier_candidate_exact_matches,
      lucky_source_attack_candidate_exact_matches:
        luckyParentMultiplierProof.source_attack_candidate_exact_matches,
      lucky_source_attack_relation_groups: luckyParentMultiplierProof.relation_groups,
      lucky_source_attribute_candidate_events:
        luckyParentMultiplierProof.source_attribute_candidate_events,
      lucky_source_attribute_candidate_pairs:
        luckyParentMultiplierProof.source_attribute_candidate_pairs,
      lucky_source_attribute_candidate_ids:
        luckyParentMultiplierProof.source_attribute_candidate_ids,
      lucky_source_attribute_candidate_exact_matches:
        luckyParentMultiplierProof.source_attribute_candidate_exact_matches,
      opaque_attribute_443_observations: rlogOpaqueAttributeAudit.attribute_443.observation_count,
      opaque_attribute_474_observations: rlogOpaqueAttributeAudit.attribute_474.observation_count,
      opaque_attribute_474_pair_entries: rlogOpaqueAttributeAudit.attribute_474.pair_entry_count,
      opaque_attribute_474_pair_entries_matching_session_entities:
        rlogOpaqueAttributeAudit.attribute_474.pair_entries_matching_session_entities,
      source_status_55342_packet_healing_results:
        sourceStatusConfounderRouteAudit.packet_healing_results,
      source_status_55342_same_context_difference_count:
        sourceStatusConfounderRouteAudit.same_context_source_status_difference_count,
      source_status_2207252_external_player_windows:
        sourceStatusLocalObservableAudit.effect_2207252_external_player_windows,
      source_status_2207252_exact_agility_delta_occurrences:
        sourceStatusLocalObservableAudit.effect_2207252_exact_agility_delta_occurrences,
      source_status_2207252_remote_provider_attribute_context_examples:
        sourceStatusLocalObservableAudit.remote_provider_attribute_context_examples,
      exact_provider_ownership_proven: ownership.unresolved === 0,
      exact_damage_projection_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    blockers: [
      ...(ownership.unresolved > 0
        ? [`${ownership.unresolved} exact status lifecycle events across ${ownershipGapWorklist.gap_groups} groups lack packet-proven player ownership`]
        : []),
      "exact packet component 282322503 and coefficient 25000 are bound for all 185 ability-823225 hit-3 packets, but coefficient-to-base placement and the armor-to-damage equation remain unproven for build 24687926",
      "exact-build aggregate offline search found no controlled target-mitigation pair; closure must use a locally observable same-capture control or an exact-build offline equation with proven combat-stage binding, never structurally unavailable remote-player packets",
      "focused exact-build captures contain no differing-defense pair even when only target status state is relaxed",
      "the exact current-season character-sheet route selects FightAttrTranTable[3].DefPara [22000,1,1,0,0,0,0] and evaluates 100*raw/(raw+22000), but combat-stage binding, effect placement, and server integer rounding remain unproven",
      "the 22000 transformed curve matches three status-confounded rows and the 6500 simple curve does not, but exact target status control is absent",
      "same-axis evidence contains one deterministic divergent damage result under a target-status change, so status confounding cannot be discarded",
      "candidate 650-basis-point defense-reduction rounding variants produce distinct packet signatures but no controlled packet selects one",
      "produced-action routing was audited for 12 target-status deltas but proves no status modifier damage-neutral",
      "effect 2201452 is proven to produce a +10 percent integer-truncating physical-defense transition, but the target defense-to-damage curve and any additional hidden damage-stage behavior remain unproven",
      "all three effect-2201452 physical-defense near-pairs remain target-status-confounded after an exhaustive 24-cohort local search; structurally unavailable remote-player packets are not a closure requirement",
      "actor/scene replay found 23 physical-defense rows for ability 823225 in one capture and zero cross-capture actor-shape pairs; the missing remote-player stable ID is preserved and not synthesized",
      `${rlogGapWindowAudit.complete_gap_bounded_lifecycle_count} exact effect-2110092 lifecycles are complete inside data-gap-bounded RLOG segments, but none supplies an otherwise-identical effect-present/effect-absent damage pair`,
      `${targetEffectFormulaProof.formula_samples} unique damage rows survive exact effect-2110092 gap-window and wire-start intersection, but ${targetEffectFormulaProof.target_physical_defense_samples} carry target physical-defense attribute 11350, so the armor-to-damage curve cannot be solved from this local cohort`,
      `${rlogTransitionCounterfactualAudit.opposite_state_recent_comparisons} transition-adjacent opposite-state comparisons produced ${rlogTransitionCounterfactualAudit.same_normalized_damage_context_pairs} same-context pairs and zero exact observed-input controls`,
      `diagnostically excluding attributes 443 and 474 plus target current HP leaves one equal-output ability-2031105 pair, but five observed status dimensions remain, source/target attribute snapshots are incomplete, and the segment status baseline is unresolved`,
      `same-build row 2203110503 is proven across ${luckyPacketComponentProof.selected_row.packet_damage_results} dedicated lucky_value packets, but MAttackLucky server-operator semantics and the physical-versus-magic mitigation route remain unproven`,
      `${mattackLuckyMitigationDiagnostic.selected_sample_count} exact-build MAttackLucky hit-3 packets were audited, but none carries an observed physical- or magic-defense axis; unavailable remote packets are not required or synthesized, and absent axes are not treated as zero mitigation`,
      `${attackLuckyMitigationDiagnostic.selected_sample_count} exact-build AttackLucky hit-3 packets were audited, but none carries an observed physical- or magic-defense axis; the physical-versus-magic route therefore remains unresolved for both Lucky families`,
      `${luckyParentMultiplierProof.lucky_events} exact current-build ability-2031109 Lucky events have one immediate same-wire parent and zero unresolved parents, but all three multiplier candidates with recorded inputs have zero exact matches across ${luckyParentMultiplierProof.multiplier_candidate_events} events; the AttrAttack candidate remains positively short by 1823 to 84858 across ${luckyParentMultiplierProof.relation_groups} exact parent/stage/critical groups`,
      "opaque attributes 443 and 474 are structurally characterized from local wire events, but exact-build semantic identities remain unproven, so neither is excluded from counterfactual matching",
      "source status 55342 has an exact healing-only produced-action route with 22320 observed healing results and zero damage results, but it differs in 33 of 37 same-context source states and has no isolated modifier-neutrality proof, so it remains a counterfactual confounder",
      "source status 2207252 has 12948 exact external player-to-player windows and 48 exact single-status recipient Agility transitions, but remote provider attribute context is structurally unavailable and the general transfer percentage, rounding, and downstream damage projection remain unproven",
      "24 exact-build counterfactual proofs contain 3009 target-locus observations of the four common status confounders and zero exact controlled target groups",
      "damage operation order and integer rounding are unproven",
      "controlled counterfactual projection is absent",
      "canonical replay conservation is unproven",
    ],
  };
}

function validateRuntimeProof(proof, build, effectId) {
  if (proof?.schema_version !== 1 || proof?.generated_by !== "tools/rdps-imagine-runtime-proof.mjs" ||
    String(proof?.game_build) !== build ||
    proof?.policy?.status_routes_require_exact_component_effect_id !== true ||
    proof?.policy?.provider_identity_requires_equipped_imagine_ability_in_that_run !== true ||
    proof?.policy?.packet_evidence_is_preserved_without_formula_or_recount_guesses !== true ||
    !Array.isArray(proof.skills)) {
    throw new Error("Runtime provider/recipient proof is not exact-build fail-closed schema-1 evidence");
  }
  const skill = exactRow(proof.skills, "imagine_skill_id", COMPOUND_SKILL_ID, "runtime Goblin March skill");
  if (Number(skill.item_id) !== PROVIDER_ITEM_ID ||
    JSON.stringify((skill.summary?.observed_tiers ?? []).map(Number)) !== JSON.stringify([5])) {
    throw new Error("Runtime Goblin March item/tier identity does not match the expected current-build observation");
  }
  const component = exactRow(skill.components ?? [], "component_id", COMPONENT_ID, "runtime Blade Sweep component");
  if (!(component.effect_ids ?? []).map(Number).includes(effectId) ||
    component.rdps_disposition !== "preserve-exact-status-window-select-owner-from-packet-never-from-shared-config" ||
    !Array.isArray(component.status_observations) || component.status_observations.length === 0) {
    throw new Error("Runtime Blade Sweep component is missing the exact fail-closed status route");
  }
  for (const observation of component.status_observations) {
    if (Number(observation.effect_id) !== effectId || Number(observation.equipped_item_id) !== PROVIDER_ITEM_ID ||
      Number(observation.equipped_tier) !== 5 || String(observation.target_kind) !== "monster" ||
      !String(observation.provider_entity_uuid ?? "")) {
      throw new Error("Runtime status observation does not bind the exact effect, item, tier, provider, and monster target");
    }
  }
  return { component, observations: component.status_observations };
}

function validateBuildSourceManifest(manifest, build, inputs) {
  if (manifest?.schemaVersion !== 1 || manifest?.generatedBy !== "tools/bpsr-build-source-manifest.mjs" ||
    String(manifest?.gameBuild) !== build || manifest?.game !== "blue-protocol-star-resonance" ||
    manifest?.deployment !== "global" || manifest?.channel !== "steam" ||
    String(manifest?.distribution?.buildId) !== build || manifest?.distribution?.snapshotPresent !== true ||
    manifest?.authority?.decodedGameTables !== "exact-current-build-static-data" ||
    manifest?.coverage?.complete !== true || Number(manifest?.coverage?.silentOmissions) !== 0 ||
    !Array.isArray(manifest.files)) {
    throw new Error("Complete build source manifest is not exhaustive exact-current-build static authority");
  }
  const required = new Map([
    ["SkillTable.json", inputs.skill_table],
    ["SkillEffectTable.json", inputs.skill_effect_table],
    ["SkillAoyiStarTable.json", inputs.aoyi_star_table],
    ["BuffTable.json", inputs.buff_table],
  ]);
  const bindings = [];
  for (const [relativePath, descriptor] of required) {
    const matches = manifest.files.filter((entry) =>
      entry.root === "decoded-game-tables" && entry.relativePath === relativePath
    );
    if (matches.length !== 1) {
      throw new Error(`Build source manifest matched ${matches.length} exact rows for ${relativePath}`);
    }
    const entry = matches[0];
    if (entry.authority !== "exact-current-build-static-data" ||
      Number(entry.bytes) !== Number(descriptor.bytes) || entry.sha256 !== descriptor.sha256) {
      throw new Error(`Decoded table ${relativePath} does not match the exact current-build manifest identity`);
    }
    bindings.push({
      relative_path: relativePath,
      manifest_id: String(entry.id),
      bytes: Number(entry.bytes),
      sha256: String(entry.sha256),
      authority: String(entry.authority),
    });
  }
  return {
    distribution_build_id: String(manifest.distribution.buildId),
    distribution_depot_manifests: structuredClone(manifest.distribution.depots ?? []),
    manifest_aggregate_sha256: String(manifest.aggregateSha256 ?? ""),
    decoded_table_bindings: bindings,
    exhaustive_source_manifest_complete: true,
    exact_static_table_hash_binding_proven: true,
  };
}

function validateOwnershipProof(proof, build, effectId) {
  const selected = Number(proof?.summary?.selected_status_events);
  const stable = Number(proof?.summary?.selected_events_with_stable_player_character_id);
  const schemaVersion = Number(proof?.schema_version);
  const priorStatusInstance = Number(
    proof?.summary?.selected_events_with_prior_status_instance_player_owner ?? 0,
  );
  const sameWirePacket = Number(
    proof?.summary?.selected_events_with_same_wire_packet_player_owner ?? 0,
  );
  if (![3, 4, 5].includes(schemaVersion) ||
    proof?.tool !== "rlogs-bpsr-status-effect-provider-ownership-proof" ||
    String(proof?.game_build) !== build ||
    proof?.policy?.exact_numeric_effect_ids_authoritative !== true ||
    (schemaVersion >= 4 &&
      (proof?.policy?.prior_exact_status_instance_player_ownership_may_flow_forward !== true ||
        proof?.policy
          ?.forward_status_instance_ownership_requires_exact_run_target_effect_instance_and_source !== true ||
        proof?.policy?.conflicting_status_instance_owners_disable_inheritance !== true)) ||
    (schemaVersion >= 5 &&
      (proof?.policy?.later_attributed_combat_relation_in_same_exact_wire_packet_may_resolve_provider !== true ||
        proof?.policy
          ?.same_wire_packet_resolution_requires_exact_capture_connection_stream_and_observed_time !== true)) ||
    proof?.policy?.future_actor_snapshots_may_backfill_prior_status_events !== false ||
    proof?.policy?.unknown_and_unresolved_events_preserved !== true ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    JSON.stringify((proof?.selection?.effect_ids ?? []).map(Number)) !== JSON.stringify([effectId]) ||
    !Number.isSafeInteger(selected) || !Number.isSafeInteger(stable) || selected < stable ||
    !Number.isSafeInteger(priorStatusInstance) || priorStatusInstance < 0 ||
    !Number.isSafeInteger(sameWirePacket) || sameWirePacket < 0 ||
    !Array.isArray(proof.inputs) || proof.inputs.length === 0) {
    throw new Error("Provider ownership proof is not incomplete exact-build schema-3 evidence for this effect");
  }
  const rlogs = proof.inputs.map(validateRlogDescriptor);
  return {
    selected,
    stable,
    priorStatusInstance,
    sameWirePacket,
    unresolved: selected - stable,
    rlogs,
  };
}

function validateOwnershipGapWorklist(worklist, build, effectId, ownershipProofInput, ownership) {
  const schemaVersion = Number(worklist?.schema_version);
  if (![1, 2, 3].includes(schemaVersion) ||
    worklist?.generated_by !== "tools/bpsr-provider-ownership-gap-worklist.mjs" ||
    String(worklist?.game_build) !== build || Number(worklist?.effect_id) !== effectId ||
    worklist?.content_sha256 !== orderedContentHash(worklist) ||
    worklist?.policy?.provider_ownership_proof_is_the_only_resolution_authority !== true ||
    worklist?.policy?.later_or_separate_same_source_rows_are_diagnostic_only !== true ||
    (schemaVersion >= 2 &&
      worklist?.policy?.prior_exact_status_instance_player_ownership_may_flow_forward_only !== true) ||
    (schemaVersion >= 3 &&
      (worklist?.policy
        ?.exact_same_wire_packet_attributed_combat_relations_may_resolve_earlier_emitted_statuses !== true ||
        worklist?.policy
          ?.same_wire_packet_resolution_requires_exact_capture_connection_stream_and_observed_time !== true)) ||
    worklist?.policy?.future_actor_or_ownership_evidence_may_backfill_prior_status_events !== false ||
    worklist?.policy?.unresolved_events_are_preserved !== true ||
    worklist?.policy?.formula_authority !== false || worklist?.policy?.runtime_authority !== false ||
    worklist?.policy?.provider_rdps_credit_allowed !== false) {
    throw new Error("Provider ownership gap worklist is not exact-build fail-closed schema-1 evidence");
  }
  const boundProof = worklist.input?.provider_ownership_proof;
  if (!boundProof || Number(boundProof.bytes) !== Number(ownershipProofInput.bytes) ||
    String(boundProof.sha256) !== String(ownershipProofInput.sha256)) {
    throw new Error("Provider ownership gap worklist is not bound to the selected ownership proof");
  }
  const summary = worklist.summary ?? {};
  const sameSourceDiagnostic = Number(
    summary.unresolved_events_with_same_source_separate_stable_player_resolution,
  );
  const withoutSameSource = Number(
    summary.unresolved_events_without_same_source_stable_player_resolution,
  );
  const gapGroups = Number(summary.gap_groups);
  const exactOwnership = ownership.unresolved === 0;
  if (Number(summary.selected_status_events) !== ownership.selected ||
    Number(summary.stable_player_owned_status_events) !== ownership.stable ||
    (schemaVersion >= 2 &&
      Number(summary.prior_status_instance_player_owned_status_events) !==
        ownership.priorStatusInstance) ||
    (schemaVersion >= 3 &&
      Number(summary.same_wire_packet_player_owned_status_events) !== ownership.sameWirePacket) ||
    Number(summary.unresolved_status_events) !== ownership.unresolved ||
    !Number.isSafeInteger(gapGroups) || gapGroups < 0 ||
    (exactOwnership ? gapGroups !== 0 : gapGroups <= 0) ||
    !Number.isSafeInteger(sameSourceDiagnostic) || sameSourceDiagnostic < 0 ||
    !Number.isSafeInteger(withoutSameSource) || withoutSameSource < 0 ||
    sameSourceDiagnostic + withoutSameSource !== ownership.unresolved ||
    summary.exact_provider_ownership_proven !== exactOwnership ||
    summary.acquisition_required !== !exactOwnership ||
    summary.formula_authority !== false || summary.runtime_authority !== false ||
    summary.provider_rdps_credit_allowed !== false ||
    !Array.isArray(worklist.acquisition_contract?.required_event_routes) ||
    worklist.acquisition_contract.required_event_routes.length === 0 ||
    !Array.isArray(worklist.acquisition_contract?.forbidden_shortcuts) ||
    worklist.acquisition_contract.forbidden_shortcuts.length === 0) {
    throw new Error("Provider ownership gap worklist counts do not reconcile with the ownership proof");
  }
  return {
    status: exactOwnership
      ? "exact-provider-ownership-proven"
      : "exact-gap-inventory-acquisition-required",
    selected_status_events: ownership.selected,
    stable_player_owned_status_events: ownership.stable,
    prior_status_instance_player_owned_status_events: ownership.priorStatusInstance,
    same_wire_packet_player_owned_status_events: ownership.sameWirePacket,
    unresolved_status_events: ownership.unresolved,
    gap_groups: gapGroups,
    unresolved_events_with_same_source_separate_stable_player_resolution: sameSourceDiagnostic,
    unresolved_events_without_same_source_stable_player_resolution: withoutSameSource,
    resolution_class_counts: structuredClone(summary.resolution_class_counts ?? {}),
    status_state_counts: structuredClone(summary.status_state_counts ?? {}),
    rlog_counts: structuredClone(summary.rlog_counts ?? {}),
    acquisition_contract: structuredClone(worklist.acquisition_contract),
    exact_provider_ownership_proven: exactOwnership,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateCounterfactualRollup(rollup, build, effectId) {
  if (rollup?.schema_version !== 1 || rollup?.generated_by !== "reviewed-status-effect-counterfactual-rollup" ||
    String(rollup?.game_build) !== build || Number(rollup?.effect_id) !== effectId ||
    rollup?.policy?.cross_session_pairing_allowed !== false ||
    rollup?.policy?.formula_authority !== false || rollup?.policy?.runtime_authority !== false ||
    rollup?.policy?.provider_rdps_credit_allowed !== false || !Array.isArray(rollup.runs) || rollup.runs.length === 0) {
    throw new Error("Counterfactual rollup is not exact-build fail-closed schema-1 evidence");
  }
  const summary = rollup.summary ?? {};
  if (Number(summary.exact_controlled_groups) !== 0 || Number(summary.exact_sample_comparisons) !== 0) {
    throw new Error("Focused proof expects the current projection frontier to remain uncontrolled");
  }
  return {
    rlogs: rollup.runs.map((run) => validateRlogDescriptor(run.rlog)),
    cohorts: rollup.runs.map((run) => validateRlogDescriptor(run.cohort)),
    matchingRuns: Number(summary.matching_capture_runs),
    formulaSamples: Number(summary.formula_damage_samples),
    sourceSamples: Number(summary.source_locus_observed_samples),
    targetSamples: Number(summary.target_locus_observed_samples),
    exactControlledGroups: Number(summary.exact_controlled_groups),
    exactSampleComparisons: Number(summary.exact_sample_comparisons),
  };
}

function validateTargetMitigationRollup(rollup, build, counterfactual) {
  if (rollup?.schema_version !== 1 ||
    rollup?.generated_by !== "tools/bpsr-target-mitigation-proof-rollup.mjs" ||
    String(rollup?.game_build) !== build || rollup?.content_sha256 !== contentHash(rollup) ||
    rollup?.policy?.every_capture_is_analyzed_independently !== true ||
    rollup?.policy?.cross_capture_pairing_allowed !== false ||
    rollup?.policy?.bounded_memory_measurement_required_for_every_input !== true ||
    rollup?.policy?.absence_of_controlled_pairs_is_not_formula_proof !== true ||
    rollup?.policy?.formula_authority !== false || rollup?.policy?.runtime_authority !== false ||
    rollup?.policy?.provider_rdps_credit_allowed !== false ||
    rollup?.status !== "no-controlled-target-mitigation-pairs" ||
    Number(rollup?.summary?.matching_build_capture_proofs) !== counterfactual.matchingRuns ||
    Number(rollup?.summary?.damage_samples) !== counterfactual.formulaSamples ||
    Number(rollup?.summary?.audited_axis_samples) <= 0 ||
    Number(rollup?.summary?.controlled_groups) !== 0 ||
    Number(rollup?.summary?.deterministic_pairs) !== 0 ||
    Number(rollup?.summary?.divergent_output_pairs) !== 0 ||
    Number(rollup?.summary?.maximum_measured_peak_working_set_bytes) <= 0 ||
    Number(rollup?.summary?.maximum_measured_peak_working_set_mib) <= 0 ||
    rollup?.summary?.exact_target_mitigation_formula_proven !== false ||
    rollup?.summary?.operation_order_and_integer_rounding_proven !== false ||
    rollup?.summary?.packet_conservation_proven !== false ||
    rollup?.summary?.formula_authority !== false || rollup?.summary?.runtime_authority !== false ||
    rollup?.summary?.provider_rdps_credit_allowed !== false || !Array.isArray(rollup.runs)) {
    throw new Error("Target mitigation rollup is not bounded exact-build fail-closed evidence");
  }
  const rollupRlogs = rollup.runs.flatMap((run) => run.cohort?.source_inputs ?? []).map((value) => ({
    path: value,
  }));
  const expectedRlogs = counterfactual.rlogs.map((entry) => ({ path: entry.path }));
  const basenames = (values) => values.map((entry) => path.basename(String(entry.path)).toLowerCase()).sort();
  if (JSON.stringify(basenames(rollupRlogs)) !== JSON.stringify(basenames(expectedRlogs))) {
    throw new Error("Target mitigation rollup does not exactly cover the counterfactual RLOG cohort");
  }
  const cohortKey = (entry) =>
    `${path.basename(String(entry.path)).toLowerCase()}|${Number(entry.bytes)}|${String(entry.sha256)}`;
  const observedCohorts = rollup.runs.map((run) => run.cohort).map(cohortKey).sort();
  const expectedCohorts = counterfactual.cohorts.map(cohortKey).sort();
  if (JSON.stringify(observedCohorts) !== JSON.stringify(expectedCohorts)) {
    throw new Error("Target mitigation rollup does not exactly match the counterfactual formula-cohort identities");
  }
  return {
    status: String(rollup.status),
    matching_build_capture_proofs: Number(rollup.summary.matching_build_capture_proofs),
    damage_samples: Number(rollup.summary.damage_samples),
    audited_axis_samples: Number(rollup.summary.audited_axis_samples),
    controlled_groups: Number(rollup.summary.controlled_groups),
    deterministic_pairs: Number(rollup.summary.deterministic_pairs),
    divergent_output_pairs: Number(rollup.summary.divergent_output_pairs),
    maximum_measured_peak_working_set_bytes:
      Number(rollup.summary.maximum_measured_peak_working_set_bytes),
    maximum_measured_peak_working_set_mib:
      Number(rollup.summary.maximum_measured_peak_working_set_mib),
    exact_target_mitigation_formula_proven: false,
    operation_order_and_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
    blockers: structuredClone(rollup.blockers ?? []),
    source_rlogs: basenames(rollupRlogs),
  };
}

function validateGlobalTargetMitigationRollup(rollup, build, focused) {
  if (rollup?.schema_version !== 1 ||
    rollup?.generated_by !== "tools/bpsr-target-mitigation-proof-rollup.mjs" ||
    String(rollup?.game_build) !== build || rollup?.content_sha256 !== contentHash(rollup) ||
    rollup?.policy?.every_capture_is_analyzed_independently !== true ||
    rollup?.policy?.cross_capture_pairing_allowed !== false ||
    rollup?.policy?.bounded_memory_measurement_required_for_every_input !== true ||
    rollup?.policy?.absence_of_controlled_pairs_is_not_formula_proof !== true ||
    rollup?.policy?.formula_authority !== false || rollup?.policy?.runtime_authority !== false ||
    rollup?.policy?.provider_rdps_credit_allowed !== false ||
    rollup?.status !== "no-controlled-target-mitigation-pairs" ||
    Number(rollup?.summary?.matching_build_source_rlogs) < focused.source_rlogs.length ||
    Number(rollup?.summary?.damage_samples) < focused.damage_samples ||
    Number(rollup?.summary?.audited_axis_samples) < focused.audited_axis_samples ||
    Number(rollup?.summary?.controlled_groups) !== 0 ||
    Number(rollup?.summary?.deterministic_pairs) !== 0 ||
    Number(rollup?.summary?.divergent_output_pairs) !== 0 ||
    Number(rollup?.summary?.maximum_measured_peak_working_set_bytes) <= 0 ||
    rollup?.summary?.exact_target_mitigation_formula_proven !== false ||
    rollup?.summary?.operation_order_and_integer_rounding_proven !== false ||
    rollup?.summary?.packet_conservation_proven !== false ||
    rollup?.summary?.formula_authority !== false || rollup?.summary?.runtime_authority !== false ||
    rollup?.summary?.provider_rdps_credit_allowed !== false || !Array.isArray(rollup.runs)) {
    throw new Error("Global target mitigation rollup is not bounded exact-build fail-closed evidence");
  }
  const sourceRlogs = rollup.runs
    .flatMap((run) => run.cohort?.source_inputs ?? [])
    .map((value) => path.basename(String(value)).toLowerCase())
    .sort();
  if (focused.source_rlogs.some((rlog) => !sourceRlogs.includes(rlog))) {
    throw new Error("Global target mitigation rollup does not contain the focused effect RLOG cohort");
  }
  return {
    status: String(rollup.status),
    matching_build_capture_proofs: Number(rollup.summary.matching_build_capture_proofs),
    matching_build_source_rlogs: Number(rollup.summary.matching_build_source_rlogs),
    cohort_input_bytes: Number(rollup.summary.cohort_input_bytes),
    damage_samples: Number(rollup.summary.damage_samples),
    audited_axis_samples: Number(rollup.summary.audited_axis_samples),
    controlled_groups: 0,
    deterministic_pairs: 0,
    divergent_output_pairs: 0,
    maximum_measured_peak_working_set_bytes:
      Number(rollup.summary.maximum_measured_peak_working_set_bytes),
    maximum_measured_peak_working_set_mib:
      Number(rollup.summary.maximum_measured_peak_working_set_mib),
    exact_target_mitigation_formula_proven: false,
    operation_order_and_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
    blockers: structuredClone(rollup.blockers ?? []),
  };
}

function validateTargetMitigationOfflineExhaustion(proof, build, globalRollupInput, globalEvidence) {
  if (proof?.schema_version !== 3 ||
    proof?.generated_by !== "tools/target-mitigation-offline-exhaustion-proof.mjs" ||
    String(proof?.game_build) !== build || String(proof?.packet_build) !== build ||
    proof?.content_sha256 !== contentHash(proof) ||
    proof?.proof_state !==
      "exact-current-build-aggregate-offline-client-and-packet-search-exhausted-final-validation-required" ||
    proof?.policy?.exact_build_required !== true ||
    proof?.policy?.exact_input_hashes_are_embedded !== true ||
    proof?.policy?.unresolved_evidence_is_hidden !== false ||
    proof?.policy?.candidate_constants_are_combat_formula_authority !== false ||
    proof?.policy?.character_sheet_transform_is_combat_formula_authority !== false ||
    proof?.policy?.absence_of_direct_calls_proves_absence_of_indirect_consumers !== false ||
    proof?.policy?.no_formula_is_promoted_without_controlled_packet_counterfactuals !== true ||
    proof?.policy?.matching_build_packet_validation_is_required !== true ||
    proof?.policy?.matching_build_formula_cohort_rollup_is_bound !== true ||
    proof?.policy?.rollup_proves_repository_wide_capture_completeness !== false) {
    throw new Error("Target mitigation offline exhaustion proof is not exact-build schema-3 fail-closed evidence");
  }
  const boundRollup = proof.inputs?.packet_pair_proof;
  if (!boundRollup || Number(boundRollup.bytes) !== Number(globalRollupInput.bytes) ||
    String(boundRollup.sha256) !== String(globalRollupInput.sha256)) {
    throw new Error("Target mitigation offline exhaustion proof is not bound to the selected global rollup");
  }
  const scope = proof.packet_evidence_scope ?? {};
  const summary = proof.summary ?? {};
  if (scope.artifact_kind !== "matching-build-formula-cohort-rollup" ||
    Number(scope.matching_build_capture_proofs) !== globalEvidence.matching_build_capture_proofs ||
    Number(scope.matching_build_source_rlogs) !== globalEvidence.matching_build_source_rlogs ||
    Number(scope.cohort_input_bytes) !== globalEvidence.cohort_input_bytes ||
    Number(scope.damage_samples) !== globalEvidence.damage_samples ||
    Number(scope.audited_axis_samples) !== globalEvidence.audited_axis_samples ||
    Number(scope.controlled_groups) !== 0 || Number(scope.deterministic_pairs) !== 0 ||
    Number(scope.divergent_output_pairs) !== 0 ||
    Number(scope.maximum_measured_peak_working_set_bytes) !==
      globalEvidence.maximum_measured_peak_working_set_bytes ||
    scope.formula_authority !== false || scope.runtime_authority !== false ||
    scope.provider_rdps_credit_allowed !== false ||
    Number(summary.packet_capture_proofs) !== globalEvidence.matching_build_capture_proofs ||
    Number(summary.packet_source_rlogs) !== globalEvidence.matching_build_source_rlogs ||
    Number(summary.packet_damage_samples) !== globalEvidence.damage_samples ||
    Number(summary.packet_audited_axis_samples) !== globalEvidence.audited_axis_samples ||
    Number(summary.packet_samples_with_physical_or_refined_defense) <= 0 ||
    Number(summary.packet_samples_with_magic_defense) < 0 ||
    Number(summary.controlled_counterfactual_pairs) !== 0 ||
    Number(summary.promoted_combat_formulas) !== 0 ||
    !Array.isArray(proof.final_validation) || proof.final_validation.length !== 2 ||
    JSON.stringify(proof.final_validation.map((entry) => String(entry.model_id)).sort()) !==
      JSON.stringify(["elemental-resistance-counterfactual", "target-physical-armor-counterfactual"])) {
    throw new Error("Target mitigation offline exhaustion counts or final acquisition contract changed");
  }
  return {
    status: String(proof.proof_state),
    packet_capture_proofs: Number(summary.packet_capture_proofs),
    packet_source_rlogs: Number(summary.packet_source_rlogs),
    packet_damage_samples: Number(summary.packet_damage_samples),
    packet_audited_axis_samples: Number(summary.packet_audited_axis_samples),
    packet_samples_with_physical_or_refined_defense:
      Number(summary.packet_samples_with_physical_or_refined_defense),
    packet_samples_with_magic_defense:
      Number(summary.packet_samples_with_magic_defense),
    packet_samples_with_elemental_defense:
      Number(summary.packet_samples_with_elemental_defense),
    controlled_counterfactual_pairs: 0,
    promoted_combat_formulas: 0,
    final_validation: structuredClone(proof.final_validation),
    exact_target_mitigation_formula_proven: false,
    operation_order_and_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetMitigationAcquisitionWorklist(worklist, build, effectId, focusedEvidence) {
  if (worklist?.schema_version !== 2 ||
    worklist?.generated_by !== "tools/bpsr-target-mitigation-acquisition-worklist.mjs" ||
    String(worklist?.game_build) !== build || Number(worklist?.effect_id) !== effectId ||
    worklist?.content_sha256 !== orderedContentHash(worklist) ||
    worklist?.status !== "acquisition-required-strict-controls-status-damage-relevance-observed" ||
    worklist?.policy?.same_capture_only !== true ||
    worklist?.policy?.cross_capture_pairing_allowed !== false ||
    worklist?.policy?.target_status_relaxation_is_diagnostic_only !== true ||
    worklist?.policy?.near_pair_is_not_controlled_counterfactual_proof !== true ||
    worklist?.policy?.unknown_and_unresolved_evidence_is_preserved !== true ||
    worklist?.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    worklist?.policy?.provider_ownership_is_already_proven_from_observable_evidence !== true ||
    worklist?.policy?.formula_authority !== false || worklist?.policy?.runtime_authority !== false ||
    worklist?.policy?.provider_rdps_credit_allowed !== false ||
    worklist?.authority?.exact_target_mitigation_formula_proven !== false ||
    worklist?.authority?.exact_operation_order_and_integer_rounding_proven !== false ||
    worklist?.authority?.packet_conservation_proven !== false ||
    worklist?.authority?.formula_authority !== false || worklist?.authority?.runtime_authority !== false ||
    worklist?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("Target mitigation acquisition worklist is not exact-build fail-closed evidence");
  }
  const scope = worklist.evidence_scope ?? {};
  if (Number(scope.matching_build_capture_diagnostics) !== focusedEvidence.matching_build_capture_proofs ||
    Number(scope.damage_samples) !== focusedEvidence.damage_samples ||
    Number(scope.audited_axis_samples) !== focusedEvidence.audited_axis_samples ||
    Number(scope.strict_controlled_groups) !== 0 ||
    Number(scope.target_status_relaxed_distinct_axis_pairs) !== 0 ||
    Number(scope.pairs_with_effect_in_target_status_delta) !== 0 ||
    Number(scope.global_same_axis_target_status_pairs) !== 5 ||
    Number(scope.global_same_axis_equal_output_pairs) !== 4 ||
    Number(scope.global_same_axis_divergent_output_pairs) !== 1 ||
    !Array.isArray(scope.matching_build_source_rlogs) ||
    JSON.stringify([...scope.matching_build_source_rlogs].sort()) !==
      JSON.stringify([...focusedEvidence.source_rlogs].sort()) ||
    !Array.isArray(worklist.acquisition_contract?.required_controls) ||
    worklist.acquisition_contract.required_controls.length === 0 ||
    !Array.isArray(worklist.acquisition_contract?.completed_prerequisites) ||
    worklist.acquisition_contract.completed_prerequisites.length !== 2 ||
    !Array.isArray(worklist.acquisition_contract?.forbidden_shortcuts) ||
    worklist.acquisition_contract.forbidden_shortcuts.length === 0) {
    throw new Error("Target mitigation acquisition worklist does not reconcile with the focused evidence");
  }
  return {
    status: String(worklist.status),
    matching_build_capture_diagnostics: Number(scope.matching_build_capture_diagnostics),
    matching_build_source_rlogs: structuredClone(scope.matching_build_source_rlogs),
    damage_samples: Number(scope.damage_samples),
    audited_axis_samples: Number(scope.audited_axis_samples),
    maximum_measured_peak_working_set_bytes:
      Number(scope.maximum_measured_peak_working_set_bytes),
    strict_controlled_groups: 0,
    target_status_relaxed_distinct_axis_pairs: 0,
    pairs_with_effect_in_target_status_delta: 0,
    global_same_axis_target_status_pairs: 5,
    global_same_axis_equal_output_pairs: 4,
    global_same_axis_divergent_output_pairs: 1,
    structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
    acquisition_contract: structuredClone(worklist.acquisition_contract),
    exact_target_mitigation_formula_proven: false,
    operation_order_and_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetMitigationNearPairCandidateProof(proof, build) {
  const sameAxis = proof?.confounders?.same_axis_status_invariance;
  if (proof?.schema_version !== 3 ||
    proof?.generated_by !== "tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs" ||
    String(proof?.game_build) !== build ||
    proof?.model_id !== "target-physical-armor-counterfactual" ||
    proof?.content_sha256 !== orderedContentHash(proof) ||
    proof?.status !== "exact-integer-candidate-compatible-status-confounded" ||
    proof?.policy?.target_status_relaxation_is_diagnostic_only !== true ||
    proof?.policy?.candidate_compatibility_is_not_formula_proof !== true ||
    proof?.policy?.candidate_rejection_is_not_operation_order_proof !== true ||
    proof?.policy?.same_axis_divergent_outcomes_preserve_status_confounders !== true ||
    proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    Number(proof?.exact_candidate_evaluation?.transformed_curve_constant) !== 22000 ||
    Number(proof?.exact_candidate_evaluation?.runtime_simple_curve_constant) !== 6500 ||
    Number(proof?.exact_candidate_evaluation?.packet_near_pair_rows) !== 3 ||
    Number(proof?.exact_candidate_evaluation?.unique_raw_and_outcome_signatures) !== 1 ||
    Number(proof?.exact_candidate_evaluation?.transformed_curve_compatible_rows) !== 3 ||
    Number(proof?.exact_candidate_evaluation?.runtime_simple_curve_compatible_rows) !== 0 ||
    JSON.stringify(proof?.exact_candidate_evaluation?.transformed_curve_unique_shared_base_values) !==
      JSON.stringify(["107006"]) ||
    proof?.exact_candidate_evaluation?.exact_target_mitigation_formula_proven !== false ||
    proof?.confounders?.selected_blade_sweep_effect_2110092_in_status_delta !== false ||
    proof?.confounders?.exact_status_state_equal !== false ||
    proof?.confounders?.effect_2201452_present_on_higher_defense_side_in_every_row !== true ||
    proof?.confounders?.effect_2201452_damage_stage_exclusivity_proven !== false ||
    Number(proof?.confounders?.counterfactual_exhaustion?.matching_build_capture_proofs) !== 24 ||
    Number(proof?.confounders?.counterfactual_exhaustion?.matching_build_source_rlogs) !== 26 ||
    Number(proof?.confounders?.counterfactual_exhaustion?.damage_samples) !== 735016 ||
    Number(proof?.confounders?.counterfactual_exhaustion?.target_locus_observed_samples) !== 3009 ||
    Number(proof?.confounders?.counterfactual_exhaustion
      ?.exact_target_locus_controlled_groups) !== 0 ||
    proof?.confounders?.counterfactual_exhaustion
      ?.every_common_confounder_observed_at_target_locus !== true ||
    proof?.confounders?.counterfactual_exhaustion
      ?.every_common_confounder_exactly_controlled_at_target_locus !== false ||
    proof?.confounders?.counterfactual_exhaustion?.common_status_confounders_eliminated !== false ||
    Number(sameAxis?.matching_build_capture_diagnostics) !== 24 ||
    Number(sameAxis?.matching_build_source_rlogs) !== 26 ||
    Number(sameAxis?.damage_samples) !== 735016 ||
    Number(sameAxis?.physical_defense_same_axis_status_pairs) !== 5 ||
    Number(sameAxis?.physical_defense_same_axis_equal_output_pairs) !== 4 ||
    Number(sameAxis?.physical_defense_same_axis_divergent_output_pairs) !== 1 ||
    JSON.stringify(sameAxis?.single_effect_equal_outcome_effect_ids) !== JSON.stringify([2203182]) ||
    JSON.stringify(sameAxis?.effects_in_divergent_joint_status_delta) !== JSON.stringify([823226, 2110093]) ||
    JSON.stringify(sameAxis?.candidate_status_effect_ids_without_same_axis_witness) !==
      JSON.stringify([55301, 2201452]) ||
    sameAxis?.target_status_can_change_damage_outside_raw_defense !== true ||
    sameAxis?.candidate_near_pair_remains_confounded !== true ||
    !Array.isArray(proof?.packet_near_pairs) || proof.packet_near_pairs.length !== 3 ||
    !Array.isArray(proof?.acquisition_contract?.required_closure) ||
    proof.acquisition_contract.required_closure.length === 0 ||
    proof?.authority?.exact_target_mitigation_formula_proven !== false ||
    proof?.authority?.exact_operation_order_and_integer_rounding_proven !== false ||
    proof?.authority?.packet_conservation_proven !== false ||
    proof?.authority?.formula_authority !== false || proof?.authority?.runtime_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("Target mitigation near-pair candidate proof is not exact-build fail-closed evidence");
  }
  return {
    status: String(proof.status),
    model_id: String(proof.model_id),
    transformed_curve_constant: 22000,
    runtime_simple_curve_constant: 6500,
    packet_near_pair_rows: 3,
    unique_raw_and_outcome_signatures: 1,
    transformed_curve_compatible_rows: 3,
    transformed_curve_unique_shared_base_values: ["107006"],
    runtime_simple_curve_compatible_rows: 0,
    selected_blade_sweep_effect_2110092_in_status_delta: false,
    exact_status_state_equal: false,
    effect_2201452_damage_stage_exclusivity_proven: false,
    confounder_counterfactual_exhaustion:
      structuredClone(proof.confounders.counterfactual_exhaustion),
    same_axis_status_invariance: structuredClone(sameAxis),
    acquisition_contract: structuredClone(proof.acquisition_contract),
    exact_target_mitigation_formula_proven: false,
    operation_order_and_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetDefenseTransformBoundary(
  surface,
  evaluator,
  seasonRoute,
  build,
  evaluatorInput,
) {
  const formula =
    "100 * raw * p3 / (raw * p2 + p1 + min(season_level * p4, p5) + min(role_level * p6, p7))";
  const parameters = [22000, 1, 1, 0, 0, 0, 0];
  const surfaceRow = surface?.rows?.["3"];
  const evaluatorRow = (evaluator?.rows ?? []).find((row) => Number(row?.season_id) === 3);
  const evaluatorDescriptor = seasonRoute?.inputs?.fight_attribute_transform_proof;
  if (surface?.schema_version !== 1 ||
    surface?.generated_by !== "rlogs-bpsr-fight-attribute-transform-surface" ||
    String(surface?.game_build) !== build ||
    surface?.table?.name !== "FightAttrTranTable" ||
    Number(surface?.table?.row_count) !== 3 ||
    surface?.table?.all_primary_keys_match_object_keys !== true ||
    surface?.table?.all_required_fields_present !== true ||
    surface?.table?.all_required_field_types_valid !== true ||
    surface?.policy?.runtime_formula_authority !== false ||
    surface?.policy?.exact_row_selection_requires_packet_proof !== true ||
    surface?.policy?.curve_evaluation_requires_packet_proof !== true ||
    surface?.policy?.rounding_requires_packet_proof !== true ||
    surface?.policy?.cross_stage_ordering_requires_packet_proof !== true ||
    Number(surfaceRow?.Id) !== 3 ||
    JSON.stringify(surfaceRow?.DefPara) !== JSON.stringify(parameters) ||
    evaluator?.schema_version !== 1 ||
    evaluator?.generated_by !== "tools/fight-attribute-transform-evaluator-proof.mjs" ||
    String(evaluator?.game_build) !== build ||
    evaluator?.proof_state !== "exact-current-build-client-ui-evaluator" ||
    evaluator?.policy?.formula_operation_order_is_exact !== true ||
    evaluator?.policy?.table_parameter_values_are_exact !== true ||
    evaluator?.policy?.ui_display_truncation_is_not_runtime_counterfactual_rounding !== true ||
    evaluator?.policy?.combat_damage_stage_authority !== false ||
    evaluator?.summary?.evaluator_formula !== formula ||
    evaluator?.summary?.row_selection !== "FightAttrTranTable[current_season_id]" ||
    evaluator?.summary?.underlying_value_rounding !== "no rounding in the proven evaluator" ||
    evaluator?.summary?.display_only_rounding !== "value - (value % 0.01)" ||
    evaluatorRow?.fields?.DefPara?.state !== "exact-current-build-parameter-array" ||
    JSON.stringify(evaluatorRow?.fields?.DefPara?.parameters) !== JSON.stringify(parameters) ||
    evaluatorRow?.fields?.DefPara?.exact_expression !== formula ||
    seasonRoute?.schema_version !== 1 ||
    seasonRoute?.generated_by !== "tools/bpsr-mastery-runtime-route-proof.mjs" ||
    String(seasonRoute?.game_build) !== build ||
    seasonRoute?.content_sha256 !== orderedContentHash(seasonRoute) ||
    seasonRoute?.proof_state !== "exact-current-build-canonical-runtime-input-route-proven" ||
    seasonRoute?.policy?.combat_damage_stage_authority_remains_unproven !== true ||
    Number(seasonRoute?.transform_contract?.current_season_id) !== 3 ||
    seasonRoute?.transform_contract?.combat_damage_stage_authority !== false ||
    seasonRoute?.transform_contract?.rounding_scope !==
      "UI display rounding only; not a runtime counterfactual rounding rule" ||
    !evaluatorDescriptor || !evaluatorInput ||
    Number(evaluatorDescriptor.bytes) !== Number(evaluatorInput.bytes) ||
    String(evaluatorDescriptor.sha256) !== String(evaluatorInput.sha256)) {
    throw new Error("Target defense transform boundary is not exact-build fail-closed evidence");
  }
  return {
    status: "exact-current-season-character-sheet-defense-transform-combat-stage-unbound",
    current_season_id: 3,
    table: "FightAttrTranTable",
    field: "DefPara",
    parameters,
    exact_evaluator_formula: formula,
    exact_current_season_expression: "100 * raw / (raw + 22000)",
    exact_current_season_curve_constant: 22000,
    character_sheet_row_selection_proven: true,
    character_sheet_operation_order_proven: true,
    character_sheet_underlying_rounding: "none",
    character_sheet_display_truncation: "value - (value % 0.01)",
    combat_stage_binding_proven: false,
    effect_reduces_raw_defense_before_transform_proven: false,
    server_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_rdps_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateCounterfactualDiscriminants(proof, build, effectId) {
  const rows = proof?.exact_discriminant_rows;
  const packetFormulaIdentity = proof?.packet_formula_identity;
  const observedBaselineCurve = proof?.observed_baseline_curve;
  if (Number(proof?.schema_version) !== 3 ||
    proof?.generated_by !== "tools/bpsr-blade-sweep-counterfactual-discriminants.mjs" ||
    String(proof?.game_build) !== build || Number(proof?.effect_id) !== effectId ||
    proof?.content_sha256 !== orderedContentHash(proof) ||
    proof?.status !== "exact-candidate-discriminants-awaiting-controlled-packet-proof" ||
    proof?.policy?.candidate_rounding_variants_are_enumerated_not_selected !== true ||
    proof?.policy?.exact_packet_component_and_static_coefficient_identity_are_proven !== true ||
    proof?.policy?.coefficient_identity_does_not_prove_defense_curve_or_formula_stage !== true ||
    proof?.policy?.three_point_integer_curve_compatibility_is_not_causal_formula_proof !== true ||
    proof?.policy?.same_input_status_invariance_is_context_bounded_not_global_formula_authority !== true ||
    proof?.policy?.ordinary_damage_and_candidate_redistribution_conserve_per_event !== true ||
    proof?.policy?.structurally_unobservable_remote_player_packets_are_not_required !== true ||
    proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
    proof?.policy?.ui_display_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    Number(proof?.proven_inputs?.armor_penetration_basis_points) !== 650 ||
    proof?.proven_inputs?.provider_ownership_proven !== true ||
    packetFormulaIdentity?.status !==
      "exact-build-packet-occurrence-static-route-and-coefficient-bound" ||
    Number(packetFormulaIdentity?.ability_id) !== 823225 ||
    Number(packetFormulaIdentity?.hit_event_id) !== 3 ||
    Number(packetFormulaIdentity?.packet_damage_source_id) !== 2 ||
    Number(packetFormulaIdentity?.damage_attr_id) !== 282322503 ||
    JSON.stringify(packetFormulaIdentity?.pve_damage_ratio_basis_points) !== JSON.stringify([25000]) ||
    Number(packetFormulaIdentity?.packet_damage_results) !== 185 ||
    packetFormulaIdentity?.exact_packet_occurrence_proven !== true ||
    packetFormulaIdentity?.exact_static_damage_row_selection_proven !== true ||
    packetFormulaIdentity?.exact_coefficient_identity_proven !== true ||
    packetFormulaIdentity?.coefficient_to_pre_mitigation_base_formula_proven !== false ||
    packetFormulaIdentity?.defense_stage_operation_order_proven !== false ||
    observedBaselineCurve?.status !==
      "three-distinct-defense-points-share-exact-integer-base-status-control-absent" ||
    Number(observedBaselineCurve?.packet_rows) !== 185 ||
    Number(observedBaselineCurve?.target_physical_defense_rows) !== 23 ||
    Number(observedBaselineCurve?.exact_curve_compatible_rows) !== 22 ||
    Number(observedBaselineCurve?.preserved_status_confounded_rows) !== 1 ||
    JSON.stringify(observedBaselineCurve?.selected_points) !== JSON.stringify([
      { physical_defense_raw: 5367, normal_value: 86020, packet_rows: 2 },
      { physical_defense_raw: 5370, normal_value: 86011, packet_rows: 16 },
      { physical_defense_raw: 5907, normal_value: 84356, packet_rows: 4 },
    ]) ||
    Number(observedBaselineCurve?.candidate_curve_constant) !== 22000 ||
    Number(observedBaselineCurve?.unique_shared_nonnegative_base) !== 107006 ||
    observedBaselineCurve?.exact_integer_floor_compatibility_proven_for_selected_points !== true ||
    observedBaselineCurve?.selected_points_share_exact_source_attribute_state !== true ||
    observedBaselineCurve?.selected_points_share_exact_source_status_state !== true ||
    observedBaselineCurve?.selected_points_share_exact_target_status_state !== false ||
    Number(observedBaselineCurve?.same_input_status_invariance
      ?.compatible_target_status_state_ids) !== 20 ||
    Number(observedBaselineCurve?.same_input_status_invariance
      ?.common_effect_ids_across_all_compatible_rows?.length) !== 78 ||
    Number(observedBaselineCurve?.same_input_status_invariance
      ?.varying_effect_ids_across_all_compatible_rows?.length) !== 36 ||
    Number(observedBaselineCurve?.same_input_status_invariance
      ?.isolated_single_effect_toggle_count) !== 1 ||
    Number(observedBaselineCurve?.same_input_status_invariance
      ?.same_input_groups?.[1]?.isolated_single_effect_toggle_receipts?.[0]?.effect_id) !== 2203182 ||
    observedBaselineCurve?.same_input_status_invariance
      ?.common_target_status_confounders_remain !== true ||
    observedBaselineCurve?.same_input_status_invariance?.target_status_control_proven !== false ||
    observedBaselineCurve?.target_status_control_proven !== false ||
    observedBaselineCurve?.exact_target_mitigation_formula_proven !== false ||
    Number(proof?.candidate_transform?.defense_curve_constant) !== 22000 ||
    proof?.candidate_transform?.hypothesis_proven !== false ||
    proof?.candidate_transform?.operation_order_proven !== false ||
    proof?.candidate_transform?.integer_rounding_proven !== false ||
    !Array.isArray(rows) || rows.length !== 2 ||
    JSON.stringify(rows.map((row) => row.target_physical_defense_raw_without_effect)) !==
      JSON.stringify([5907, 5370]) ||
    JSON.stringify(rows.map((row) => row.distinct_predicted_damage_with_effect)) !==
      JSON.stringify([[85530, 85533], [87122, 87125]]) ||
    rows.some((row) => !Array.isArray(row.variants) || row.variants.length !== 3 ||
      row.variants.some((variant) =>
        Number(variant.recipient_counterfactual_damage_without_effect) +
          Number(variant.provider_candidate_contribution_damage) !==
          Number(variant.conserved_ordinary_damage))) ||
    proof?.acquisition_contract?.remote_player_packet_dependency !== false ||
    proof?.authority?.exact_damage_projection_proven !== false ||
    proof?.authority?.formula_authority !== false || proof?.authority?.runtime_authority !== false ||
    proof?.authority?.ui_display_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("Blade Sweep counterfactual discriminants are not exact fail-closed evidence");
  }
  return {
    status: String(proof.status),
    armor_penetration_basis_points: 650,
    defense_curve_constant: 22000,
    packet_formula_identity: structuredClone(packetFormulaIdentity),
    observed_baseline_curve: structuredClone(observedBaselineCurve),
    exact_discriminant_rows: structuredClone(rows),
    distinct_predicted_damage_with_effect: [...new Set(rows.flatMap(
      (row) => row.distinct_predicted_damage_with_effect.map(Number),
    ))].sort((left, right) => left - right),
    acquisition_contract: structuredClone(proof.acquisition_contract),
    exact_damage_projection_proven: false,
    exact_operation_order_proven: false,
    exact_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetStatusActionRouteAudit(proof, build) {
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "tools/bpsr-target-status-action-route-audit.mjs" ||
    String(proof?.game_build) !== build || proof?.content_sha256 !== orderedContentHash(proof) ||
    proof?.status !== "exact-produced-action-routes-audited-status-modifier-neutrality-unproven" ||
    proof?.policy
      ?.produced_action_healing_only_does_not_prove_status_modifier_damage_neutrality !== true ||
    proof?.policy?.no_observed_produced_action_is_not_zero_effect_proof !== true ||
    proof?.policy?.unknown_and_unresolved_status_roles_are_preserved !== true ||
    Number(proof?.summary?.audited_effects) !== 12 ||
    Number(proof?.summary?.produced_damage_action_effects) !== 0 ||
    Number(proof?.summary?.produced_action_healing_only_effects) !== 3 ||
    Number(proof?.summary?.no_produced_action_observed_effects) !== 9 ||
    Number(proof?.summary?.effects_eliminated_as_damage_neutral) !== 0 ||
    JSON.stringify(proof?.summary?.candidate_near_pair_status_effects_without_same_axis_witness) !==
      JSON.stringify([55301, 2201452]) ||
    proof?.conclusion?.candidate_near_pair_status_confounders_eliminated !== false ||
    proof?.authority?.status_modifier_damage_neutrality_proven !== false ||
    proof?.authority?.target_status_confounders_eliminated !== false ||
    proof?.authority?.exact_target_mitigation_formula_proven !== false ||
    proof?.authority?.formula_authority !== false || proof?.authority?.runtime_authority !== false ||
    proof?.authority?.ui_display_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("target-status action-route audit is not exact fail-closed evidence");
  }
  return {
    status: String(proof.status),
    audited_effects: 12,
    produced_damage_action_effects: 0,
    produced_action_healing_only_effects: 3,
    no_produced_action_observed_effects: 9,
    effects_eliminated_as_damage_neutral: 0,
    candidate_near_pair_status_effects_without_same_axis_witness: [55301, 2201452],
    exact_effect_55301_produced_action_packet_evidence:
      String(proof.conclusion.exact_effect_55301_produced_action_packet_evidence),
    exact_effect_2201452_produced_action_packet_evidence:
      String(proof.conclusion.exact_effect_2201452_produced_action_packet_evidence),
    status_modifier_damage_neutrality_proven: false,
    target_status_confounders_eliminated: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetDefensePercentLifecycleProof(proof, build) {
  const raw = proof?.packet_raw_percent_proof;
  if (Number(proof?.schema_version) !== 2 ||
    proof?.generated_by !== "tools/bpsr-defense-percent-lifecycle-proof.mjs" ||
    String(proof?.game_build) !== build ||
    Number(proof?.effect_id) !== 2201452 ||
    Number(proof?.attribute_id) !== 11350 ||
    proof?.content_sha256 !== `sha256:${contentHash(proof)}` ||
    proof?.policy?.exact_numeric_effect_attribute_and_build_identity_are_authoritative !== true ||
    proof?.policy?.localized_names_are_evidence_only !== true ||
    proof?.policy?.exact_wire_single_effect_equations_required !== true ||
    proof?.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    proof?.policy?.local_recipient_attribute_observations_are_sufficient_for_this_stat_formula !== true ||
    proof?.policy?.final_only_observations_remain_visible_and_do_not_imply_raw_field_observation !== true ||
    proof?.policy?.stat_formula_proof_does_not_grant_damage_formula_authority !== true ||
    proof?.build_identity?.preflight_ready_for_snapshot !== false ||
    proof?.build_identity?.runtime_promotion_allowed !== false ||
    Number(proof?.exact_effect_identity?.buff_table_id) !== 2201452 ||
    Number(proof?.exact_effect_identity?.origin_source_type_id) !== 1 ||
    Number(proof?.exact_effect_identity?.origin_source_config_id) !== 50049 ||
    proof?.formula?.operation !== "integer-truncating multiplicative percent increase" ||
    proof?.formula?.expression !==
      "buffed_physical_defense = trunc(base_physical_defense * (10000 + percent_basis_points) / 10000)" ||
    Number(proof?.formula?.scale) !== 10000 ||
    Number(proof?.formula?.percent_basis_points) !== 1000 ||
    Number(proof?.formula?.compatible_integer_basis_point_interval?.minimum) !== 1000 ||
    Number(proof?.formula?.compatible_integer_basis_point_interval?.maximum) !== 1000 ||
    proof?.formula?.multiplication_before_integer_truncation_proven !== true ||
    proof?.formula?.application_and_removal_replay_exact !== true ||
    Number(raw?.final_attribute_id) !== 11350 ||
    Number(raw?.intermediate_attribute_id) !== 11351 ||
    Number(raw?.base_attribute_id) !== 11352 ||
    Number(raw?.raw_extra_add_attribute_id) !== 11353 ||
    Number(raw?.raw_percent_attribute_id) !== 11354 ||
    Number(raw?.raw_extra_percent_attribute_id) !== 11355 ||
    Number(raw?.scale) !== 10000 ||
    Number(raw?.exact_family_input_transitions) !== 158 ||
    Number(raw?.exact_intermediate_formula_matches) !== 158 ||
    Number(raw?.intermediate_formula_residual_mismatches) !== 0 ||
    Number(raw?.nearest_rounding_residual_mismatches) !== 86 ||
    raw?.truncation_selected_over_round_to_nearest !== true ||
    Number(raw?.joined_exact_single_effect_occurrences) !== 47 ||
    Number(raw?.unresolved_final_only_occurrences) !== 4 ||
    raw?.all_joined_raw_percent_deltas_equal_effect_basis_points !== true ||
    raw?.raw_percent_identity_for_all_lifecycle_occurrences_proven !== false ||
    raw?.raw_extra_percent_packet_known_for_exact_family_inputs !== false ||
    raw?.joined_witnesses?.length !== 47 ||
    raw?.unresolved_final_only_witnesses?.length !== 4 ||
    Number(proof?.summary?.exact_wire_occurrences) !== 51 ||
    Number(proof?.summary?.application_occurrences) !== 30 ||
    Number(proof?.summary?.removal_occurrences) !== 21 ||
    Number(proof?.summary?.independent_sessions) !== 13 ||
    Number(proof?.summary?.distinct_external_sources) !== 3 ||
    Number(proof?.summary?.distinct_base_values) !== 5 ||
    proof?.summary?.exact_defense_stat_formula_proven !== true ||
    proof?.summary?.exact_target_defense_to_damage_formula_proven !== false ||
    proof?.summary?.provider_rdps_credit_allowed !== false ||
    proof?.summary?.ui_rdps_display_authority !== false ||
    proof?.downstream_damage_gate?.status !==
      "defense-stat-formula-proven-damage-counterfactual-unproven" ||
    proof?.downstream_damage_gate?.hidden_additional_damage_stage_behavior_excluded !== false ||
    proof?.downstream_damage_gate?.runtime_authority !== false ||
    proof?.downstream_damage_gate?.provider_rdps_credit_allowed !== false) {
    throw new Error("target defense percent lifecycle proof is not exact-build fail-closed evidence");
  }
  return {
    status: String(proof.downstream_damage_gate.status),
    effect_id: 2201452,
    attribute_id: 11350,
    percent_basis_points: 1000,
    percent: 10,
    formula: String(proof.formula.expression),
    exact_wire_occurrences: 51,
    packet_raw_percent_joined_occurrences: 47,
    final_only_unresolved_occurrences: 4,
    exact_family_input_transitions: 158,
    nearest_rounding_residual_mismatches: 86,
    truncation_selected_over_round_to_nearest: true,
    raw_percent_identity_for_all_lifecycle_occurrences_proven: false,
    application_occurrences: 30,
    removal_occurrences: 21,
    independent_sessions: 13,
    distinct_external_sources: 3,
    distinct_base_values: 5,
    effect_2201452_exact_defense_axis_mechanism_proven: true,
    exact_target_defense_to_damage_formula_proven: false,
    effect_2201452_damage_stage_exclusivity_proven: false,
    hidden_additional_damage_stage_behavior_excluded: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetDefenseFightAttributeScopeProof(proof, build) {
  const proven = proof?.proven_packet_observable_components?.[0];
  const co = proof?.raw_armor_to_crit_co_transition_test;
  if (Number(proof?.schema_version) !== 2 ||
    proof?.generated_by !== "tools/bpsr-effect-fight-attribute-scope-proof.mjs" ||
    String(proof?.game_build) !== build || Number(proof?.effect_id) !== 2201452 ||
    proof?.content_sha256 !== `sha256:${contentHash(proof)}` ||
    proof?.policy?.exact_numeric_effect_attribute_and_build_identity_are_authoritative !== true ||
    proof?.policy?.all_exact_build_fight_attribute_components_are_selected !== true ||
    proof?.policy?.same_wire_correlation_is_not_causation_without_reversible_constant_replay !== true ||
    proof?.policy?.one_direction_constant_correlations_remain_unresolved !== true ||
    proof?.policy?.sparse_one_direction_co_updates_do_not_establish_an_unconditional_component !== true ||
    proof?.policy?.nonstationary_correlations_remain_visible !== true ||
    proof?.policy?.absence_of_an_observed_fight_attribute_component_does_not_exclude_hidden_damage_logic !== true ||
    proof?.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    Number(proof?.summary?.selected_fight_attribute_components) !== 906 ||
    Number(proof?.summary?.components_with_exact_single_effect_same_wire_correlations) !== 26 ||
    Number(proof?.summary?.proven_reversible_constant_components) !== 1 ||
    Number(proof?.summary?.unresolved_one_direction_constant_components) !== 3 ||
    Number(proof?.summary?.unresolved_nonstationary_components) !== 22 ||
    Number(proven?.attribute_id) !== 11354 || proven?.component_field !== "AttrPer" ||
    JSON.stringify(proven?.normalized_coefficients) !== JSON.stringify([1000]) ||
    Number(proven?.exact_single_effect_occurrences) !== 47 ||
    Number(proven?.independent_sessions) !== 13 ||
    JSON.stringify((proof?.unresolved_one_direction_constant_correlations ?? [])
      .map((row) => Number(row.attribute_id)).sort((a, b) => a - b)) !==
      JSON.stringify([11710, 11711, 11712]) ||
    Number(co?.raw_armor_percent_attribute_id) !== 11354 ||
    Number(co?.raw_crit_add_attribute_id) !== 11712 ||
    Number(co?.exact_raw_armor_presence_transitions) !== 47 ||
    Number(co?.raw_armor_applications) !== 26 || Number(co?.raw_armor_removals) !== 21 ||
    Number(co?.applications_with_raw_crit_add_update) !== 0 ||
    Number(co?.removals_with_raw_crit_add_update) !== 2 ||
    Number(co?.exact_raw_armor_transitions_without_raw_crit_add_co_update) !== 45 ||
    Number(co?.observed_removal_only_raw_crit_add_delta) !== 50 ||
    co?.unconditional_fixed_negative_50_raw_crit_add_component_supported !== false ||
    co?.conditional_or_indirect_crit_behavior_excluded !== false ||
    co?.co_update_witnesses?.length !== 2 ||
    proof?.conclusion?.direct_raw_percent_identity_proven !== true ||
    proof?.conclusion?.effect_is_defense_stat_only_across_observed_fight_attribute_components_proven !== false ||
    proof?.conclusion?.hidden_damage_stage_behavior_excluded !== false ||
    proof?.conclusion?.formula_authority !== false || proof?.conclusion?.runtime_authority !== false ||
    proof?.conclusion?.ui_display_authority !== false ||
    proof?.conclusion?.provider_rdps_credit_allowed !== false) {
    throw new Error("target defense fight-attribute scope proof is not exact-build fail-closed evidence");
  }
  return {
    status: "complete-observed-fight-attribute-scope-hidden-damage-logic-unexcluded",
    effect_id: 2201452,
    selected_fight_attribute_components: 906,
    components_with_exact_single_effect_same_wire_correlations: 26,
    proven_reversible_constant_components: 1,
    unresolved_one_direction_constant_components: 3,
    unresolved_nonstationary_components: 22,
    unresolved_fight_attribute_components: 25,
    only_proven_reversible_constant_attribute_id: 11354,
    raw_percent_basis_points_per_effect_presence: 1000,
    raw_percent_exact_occurrences: 47,
    raw_percent_independent_sessions: 13,
    unresolved_one_direction_attribute_ids: [11710, 11711, 11712],
    raw_armor_presence_transitions: 47,
    raw_armor_applications: 26,
    raw_armor_removals: 21,
    raw_crit_add_application_co_updates: 0,
    raw_crit_add_removal_co_updates: 2,
    raw_armor_transitions_without_raw_crit_co_update: 45,
    removal_only_raw_crit_add_delta: 50,
    unconditional_fixed_negative_50_raw_crit_add_component_supported: false,
    conditional_or_indirect_crit_behavior_excluded: false,
    effect_is_defense_stat_only_across_observed_fight_attribute_components_proven: false,
    hidden_damage_stage_behavior_excluded: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetDefenseStatusDiagnosticRollup(proof, build) {
  const summary = proof?.summary;
  const transformed = proof?.physical_defense_candidate_evaluation?.["22000"];
  const simple = proof?.physical_defense_candidate_evaluation?.["6500"];
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "tools/bpsr-target-mitigation-status-diagnostic-rollup.mjs" ||
    String(proof?.game_build) !== build || Number(proof?.selected_effect_id) !== 2201452 ||
    proof?.content_sha256 !== orderedContentHash(proof) ||
    proof?.policy?.exact_numeric_effect_ids_attribute_ids_and_build_are_authoritative !== true ||
    proof?.policy?.target_status_relaxation_is_diagnostic_only !== true ||
    proof?.policy?.near_pair_is_not_controlled_counterfactual_proof !== true ||
    proof?.policy?.absence_of_additional_local_pairs_is_not_formula_proof !== true ||
    proof?.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    proof?.policy?.acquisition_must_use_locally_observable_events_or_offline_exact_build_evidence !== true ||
    Number(summary?.matching_build_capture_diagnostics) !== 24 ||
    Number(summary?.unique_cohort_inputs) !== 24 || Number(summary?.unique_source_rlogs) !== 26 ||
    Number(summary?.damage_samples) !== 735016 || Number(summary?.audited_axis_samples) !== 2294 ||
    Number(summary?.physical_defense_axis_samples) !== 765 ||
    Number(summary?.physical_defense_unique_near_pairs) !== 3 ||
    Number(summary?.physical_defense_same_axis_status_pairs) !== 5 ||
    Number(summary?.physical_defense_pairs_with_selected_effect_in_status_delta) !== 3 ||
    Number(summary?.physical_defense_same_axis_pairs_with_selected_effect_in_status_delta) !== 0 ||
    Number(summary?.diagnostics_with_physical_defense_near_pairs) !== 1 ||
    Number(summary?.maximum_measured_peak_working_set_bytes) <= 0 ||
    summary?.exhaustive_local_search_added_independent_near_pair_cohorts !== false ||
    Number(transformed?.evaluated_unique_near_pairs) !== 3 ||
    Number(transformed?.compatible_unique_near_pairs) !== 3 ||
    Number(transformed?.rejected_unique_near_pairs) !== 0 || transformed?.formula_authority !== false ||
    Number(simple?.evaluated_unique_near_pairs) !== 3 ||
    Number(simple?.compatible_unique_near_pairs) !== 0 ||
    Number(simple?.rejected_unique_near_pairs) !== 3 || simple?.formula_authority !== false ||
    proof?.conclusions?.selected_effect_occurs_in_every_observed_physical_defense_near_pair !== true ||
    proof?.conclusions?.selected_effect_has_same_axis_damage_witness !== false ||
    proof?.conclusions?.observed_near_pairs_remain_target_status_confounded !== true ||
    proof?.conclusions?.no_new_independent_local_control_was_found !== true ||
    proof?.conclusions?.remote_player_packet_acquisition_required !== false ||
    summary?.exact_target_mitigation_formula_proven !== false ||
    summary?.exact_operation_order_and_integer_rounding_proven !== false ||
    summary?.packet_conservation_proven !== false || summary?.formula_authority !== false ||
    summary?.runtime_authority !== false || summary?.ui_display_authority !== false ||
    summary?.provider_rdps_credit_allowed !== false) {
    throw new Error("target defense status diagnostic rollup is not exact-build fail-closed evidence");
  }
  return {
    status: "exhaustive-local-status-diagnostic-search-no-independent-control",
    effect_id: 2201452,
    matching_build_capture_diagnostics: 24,
    unique_source_rlogs: 26,
    damage_samples: 735016,
    audited_axis_samples: 2294,
    physical_defense_axis_samples: 765,
    physical_defense_unique_near_pairs: 3,
    physical_defense_same_axis_status_pairs: 5,
    physical_defense_pairs_with_selected_effect_in_status_delta: 3,
    physical_defense_same_axis_pairs_with_selected_effect_in_status_delta: 0,
    diagnostics_with_physical_defense_near_pairs: 1,
    transformed_curve_constant: 22000,
    transformed_curve_compatible_rows: 3,
    runtime_simple_curve_constant: 6500,
    runtime_simple_curve_compatible_rows: 0,
    selected_effect_occurs_in_every_observed_physical_defense_near_pair: true,
    selected_effect_has_same_axis_damage_witness: false,
    no_new_independent_local_control_was_found: true,
    remote_player_packet_acquisition_required: false,
    exact_target_mitigation_formula_proven: false,
    exact_operation_order_and_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetMitigationActorSceneExhaustion(proof, build) {
  const summary = proof?.summary;
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "tools/bpsr-target-mitigation-actor-scene-exhaustion.mjs" ||
    String(proof?.game_build) !== build ||
    proof?.model_id !== "target-physical-armor-counterfactual" ||
    proof?.status !== "exact-local-actor-scene-exhausted-no-cross-capture-control" ||
    proof?.content_sha256 !== contentHash(proof) ||
    proof?.policy?.only_local_capture_and_exact_build_offline_evidence_are_required !== true ||
    proof?.policy?.structurally_unavailable_remote_player_packets_are_not_required !== true ||
    proof?.policy?.missing_stable_remote_player_identity_is_preserved_not_synthesized !== true ||
    proof?.policy?.actor_shape_grouping_is_diagnostic_only !== true ||
    Number(summary?.exact_build_source_rlogs) !== 26 ||
    Number(summary?.selected_ability_id) !== 823225 ||
    Number(summary?.selected_ability_samples) !== 185 ||
    Number(summary?.physical_defense_samples) !== 23 ||
    Number(summary?.physical_defense_capture_sessions) !== 1 ||
    Number(summary?.physical_defense_samples_with_stable_target_actor_id) !== 0 ||
    Number(summary?.cross_capture_actor_shape_pairs) !== 0 ||
    Number(summary?.same_capture_status_confounded_near_pair_rows) !== 3 ||
    Number(summary?.transformed_curve_22000_compatible_status_confounded_rows) !== 3 ||
    Number(summary?.runtime_simple_curve_6500_compatible_rows) !== 0 ||
    summary?.exact_target_mitigation_formula_proven !== false ||
    summary?.packet_conservation_proven !== false ||
    proof?.authority?.formula_authority !== false ||
    proof?.authority?.runtime_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("target mitigation actor-scene exhaustion is not exact-build fail-closed evidence");
  }
  return {
    status: proof.status,
    selected_ability_id: 823225,
    selected_ability_samples: 185,
    physical_defense_samples: 23,
    physical_defense_capture_sessions: 1,
    physical_defense_samples_with_stable_target_actor_id: 0,
    cross_capture_actor_shape_pairs: 0,
    same_capture_status_confounded_near_pair_rows: 3,
    structurally_unavailable_remote_player_packets_are_not_required: true,
    missing_stable_remote_player_identity_is_preserved_not_synthesized: true,
    exact_target_mitigation_formula_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateRlogGapWindowAudit(proof, build, effectId, globalRollup, globalRollupInput) {
  const summary = proof?.summary;
  const boundRollup = proof?.inputs?.target_mitigation_rollup;
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "rlogs-bpsr-rlog-gap-window-audit" ||
    String(proof?.game_build) !== build || Number(proof?.effect_id) !== effectId ||
    proof?.content_sha256 !== `sha256:${orderedContentHash(proof)}` ||
    proof?.policy?.sealed_rlogs_are_streamed_one_event_at_a_time !== true ||
    proof?.policy?.every_data_gap_and_recorder_pause_is_an_exclusion_boundary !== true ||
    proof?.policy?.status_lifecycles_never_cross_exclusion_or_run_boundaries !== true ||
    proof?.policy?.complete_gap_bounded_lifecycle_is_not_counterfactual_formula_proof !== true ||
    proof?.policy?.packet_absence_is_not_zero !== true ||
    proof?.policy
      ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    proof?.policy?.current_snapshots_are_never_backfilled_into_historical_windows !== true ||
    proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    !boundRollup || Number(boundRollup.bytes) !== Number(globalRollupInput.bytes) ||
    String(boundRollup.sha256) !== String(globalRollupInput.sha256) ||
    Number(summary?.source_rlog_count) !== 26 || Number(summary?.sealed_rlog_count) !== 26 ||
    Number(summary?.source_rlog_bytes) <= 0 || Number(summary?.canonical_event_count) <= 0 ||
    Number(summary?.data_gap_count) <= 0 || Number(summary?.rlogs_with_data_gaps) !== 26 ||
    Number(summary?.rlogs_without_data_gaps) !== 0 ||
    Number(summary?.selected_effect_status_event_count) !== 180 ||
    Number(summary?.selected_effect_applied_count) !== 90 ||
    Number(summary?.selected_effect_terminal_count) !== 90 ||
    Number(summary?.selected_effect_complete_gap_bounded_lifecycle_count) !== 39 ||
    Number(summary?.selected_effect_complete_windows_with_damage_count) !== 39 ||
    Number(summary?.selected_effect_damage_events_while_active) !== 2277 ||
    Number(summary?.selected_effect_lifecycles_cut_by_data_quality_boundary) !== 51 ||
    Number(summary?.selected_effect_unmatched_terminal_events) !== 51 ||
    Number(summary?.selected_effect_open_at_end_of_log) !== 0 ||
    Number(summary?.selected_effect_events_without_instance_id) !== 0 ||
    Number(summary?.selected_effect_duplicate_applications) !== 0 ||
    Number(summary?.candidate_rlog_count) !== 4 ||
    summary?.exact_gap_bounded_lifecycle_windows_identified !== true ||
    summary?.exact_damage_projection_proven !== false ||
    summary?.exact_operation_order_proven !== false ||
    summary?.exact_integer_rounding_proven !== false ||
    summary?.packet_conservation_proven !== false || summary?.formula_authority !== false ||
    summary?.runtime_authority !== false || summary?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(proof.sessions) || proof.sessions.length !== 26) {
    throw new Error("effect RLOG gap-window audit is not exact-build fail-closed evidence");
  }
  const expectedRlogs = globalRollup.runs
    .flatMap((run) => run.cohort?.source_inputs ?? [])
    .map((value) => path.basename(String(value)).toLowerCase())
    .sort();
  const actualRlogs = proof.sessions
    .map((session) => path.basename(String(session.path)).toLowerCase())
    .sort();
  if (JSON.stringify([...new Set(expectedRlogs)]) !== JSON.stringify(actualRlogs) ||
    proof.sessions.some((session) =>
      !/^(sha256:)?[0-9a-f]{64}$/.test(String(session.sealed_content_sha256 ?? "")) ||
      Number(session.event_count) <= 0 ||
      !Array.isArray(session.complete_gap_bounded_windows) ||
      session.complete_gap_bounded_windows.some((window) =>
        window.gap_bounded !== true || window.controlled_counterfactual_pair_proven !== false ||
        window.formula_authority !== false ||
        Number(window.terminal_envelope_sequence) <= Number(window.applied_envelope_sequence) ||
        Number(window.damage_events_while_active) <= 0
      )
    )) {
    throw new Error("effect RLOG gap-window audit source identities or windows are unsafe");
  }
  return {
    status: "exact-gap-bounded-lifecycles-found-counterfactual-unproven",
    source_rlog_count: 26,
    canonical_event_count: Number(summary.canonical_event_count),
    data_gap_count: Number(summary.data_gap_count),
    rlogs_with_data_gaps: 26,
    complete_gap_bounded_lifecycle_count:
      Number(summary.selected_effect_complete_gap_bounded_lifecycle_count),
    complete_windows_with_damage_count:
      Number(summary.selected_effect_complete_windows_with_damage_count),
    damage_events_while_active: Number(summary.selected_effect_damage_events_while_active),
    lifecycles_cut_by_data_quality_boundary:
      Number(summary.selected_effect_lifecycles_cut_by_data_quality_boundary),
    candidate_rlogs: proof.sessions
      .filter((session) => Number(session.selected_effect_complete_windows_with_damage_count) > 0)
      .map((session) => path.basename(String(session.path)))
      .sort(),
    complete_gap_bounded_windows: proof.sessions.flatMap((session) =>
      session.complete_gap_bounded_windows.map((window) => ({
        rlog: path.basename(String(session.path)),
        ...structuredClone(window),
      }))
    ),
    exact_damage_projection_proven: false,
    exact_operation_order_proven: false,
    exact_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetEffectFormulaProof(proof, build, effectId, gapAudit, gapAuditInput) {
  const selection = proof?.selection;
  const gap = proof?.gap_window_filter;
  const surface = proof?.formula_surface;
  const physicalDefense = (proof?.candidates ?? []).find((candidate) =>
    candidate?.name === "physical_defense" &&
    candidate?.locus === "target" &&
    Number(candidate?.attribute_id) === 11350
  );
  const sampleCount = Number(proof?.sample_count);
  const matchedUnique = Number(gap?.matched_damage_events);
  const matchedMemberships = Number(gap?.matched_window_damage_memberships);
  if (![43, 44].includes(Number(proof?.schema_version)) ||
    proof?.generated_by !== "rlogs-bpsr-state-scaling-damage-proof" ||
    String(proof?.game_build) !== build || !Array.isArray(proof?.inputs) ||
    proof.inputs.length !== gapAudit.source_rlog_count ||
    selection?.all_abilities !== false || !Array.isArray(selection?.ability_ids) ||
    selection.ability_ids.length !== 0 ||
    JSON.stringify((selection?.target_effect_ids ?? []).map(Number)) !== JSON.stringify([effectId]) ||
    selection?.formula_authority !== false ||
    Number(gap?.effect_id) !== effectId ||
    String(gap?.source_sha256) !== String(gapAuditInput.sha256) ||
    Number(gap?.complete_gap_bounded_lifecycles) !== gapAudit.complete_gap_bounded_lifecycle_count ||
    Number(gap?.audited_damage_events_while_active) !== gapAudit.damage_events_while_active ||
    matchedMemberships !== gapAudit.damage_events_while_active ||
    !Number.isSafeInteger(matchedUnique) || matchedUnique <= 0 || matchedUnique > matchedMemberships ||
    gap?.formula_authority !== false || !Number.isSafeInteger(sampleCount) || sampleCount <= 0 ||
    sampleCount > matchedUnique || Number(surface?.sample_count) !== sampleCount ||
    Number(surface?.group_count) <= 0 ||
    Number(surface?.samples_with_target_physical_defense) !== 0 ||
    !physicalDefense || Number(physicalDefense.samples_with_attribute) !== 0 ||
    physicalDefense.strict_all_observed_state?.proof_authority !== false ||
    Number(physicalDefense.strict_all_observed_state?.controlled_groups) !== 0 ||
    physicalDefense.target_current_hp_excluded_diagnostic?.proof_authority !== false ||
    Number(physicalDefense.target_current_hp_excluded_diagnostic?.controlled_groups) !== 0) {
    throw new Error("target-effect formula proof is not exact-build gap-bounded fail-closed evidence");
  }
  return {
    status: "gap-bounded-target-effect-formula-input-absent",
    source_rlog_count: proof.inputs.length,
    exact_effect_id: effectId,
    complete_gap_bounded_lifecycles: Number(gap.complete_gap_bounded_lifecycles),
    gap_audited_damage_window_memberships: Number(gap.audited_damage_events_while_active),
    gap_matched_unique_damage_events: matchedUnique,
    formula_samples: sampleCount,
    gap_rows_excluded_by_wire_start_status: matchedUnique - sampleCount,
    source_physical_attack_samples: Number(surface.samples_with_source_physical_attack),
    target_physical_defense_samples: Number(surface.samples_with_target_physical_defense),
    exact_armor_to_damage_equation_proven: false,
    exact_operation_order_proven: false,
    exact_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateRlogTransitionCounterfactualAudit(proof, build, effectId, gapAuditInput) {
  const summary = proof?.summary;
  const boundGapAudit = proof?.inputs?.gap_window_audit;
  const closest = proof?.same_context_mismatch_examples?.[0];
  if (Number(proof?.schema_version) !== 4 ||
    proof?.generated_by !== "rlogs-bpsr-rlog-transition-counterfactual-audit" ||
    String(proof?.game_build) !== build || Number(proof?.effect_id) !== effectId ||
    proof?.content_sha256 !== `sha256:${orderedContentHash(proof)}` ||
    proof?.policy?.every_data_gap_pause_and_run_boundary_resets_all_observed_state !== true ||
    proof?.policy?.only_same_segment_transition_adjacent_pairs_are_compared !== true ||
    proof?.policy?.packet_absence_is_not_zero !== true ||
    proof?.policy?.unknown_segment_baseline_statuses_are_preserved_as_unresolved !== true ||
    proof?.policy?.target_current_hp_exclusion_is_diagnostic_only !== true ||
    proof?.policy?.attribute_443_474_exclusion_is_diagnostic_only !== true ||
    proof?.policy
      ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    proof?.policy?.candidate_pairs_never_grant_formula_or_runtime_authority !== true ||
    proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
    proof?.policy?.ui_display_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    proof?.search_contract?.remote_player_packet_dependency !== false ||
    !boundGapAudit || Number(boundGapAudit.bytes) !== Number(gapAuditInput.bytes) ||
    String(boundGapAudit.sha256).replace(/^sha256:/, "") !== String(gapAuditInput.sha256).replace(/^sha256:/, "") ||
    Number(summary?.source_rlog_count) !== 26 || Number(summary?.source_rlog_bytes) !== 238642397 ||
    Number(summary?.canonical_event_count) !== 6411565 || Number(summary?.reset_boundary_count) !== 16247 ||
    Number(summary?.data_gap_count) !== 16181 || Number(summary?.run_boundary_count) !== 66 ||
    Number(summary?.damage_events) !== 735016 ||
    Number(summary?.damage_events_with_selected_effect_active) !== 5463 ||
    Number(summary?.damage_events_with_selected_effect_absent) !== 729553 ||
    Number(summary?.opposite_state_recent_comparisons) !== 47626 ||
    Number(summary?.same_normalized_damage_context_pairs) !== 37 ||
    Number(summary?.same_context_and_observed_attribute_pairs) !== 0 ||
    Number(summary?.same_context_and_nonselected_status_pairs) !== 0 ||
    Number(summary?.same_context_pairs_with_only_target_current_hp_difference) !== 0 ||
    Number(summary?.same_context_pairs_after_443_474_attribute_exclusion) !== 0 ||
    Number(summary?.same_context_pairs_after_443_474_and_target_current_hp_exclusion) !== 1 ||
    Number(summary
      ?.same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses) !== 0 ||
    Number(summary?.minimum_residual_observed_state_dimensions_after_443_474_exclusion) !== 6 ||
    Number(summary
      ?.minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion) !== 5 ||
    Number(summary?.same_context_source_attribute_difference_counts?.[474]) !== 37 ||
    Number(summary?.same_context_target_attribute_difference_counts?.[443]) !== 37 ||
    Number(summary?.same_context_target_attribute_difference_counts?.[474]) !== 37 ||
    Number(summary?.same_context_target_attribute_difference_counts?.[11310]) !== 37 ||
    Object.keys(summary?.same_context_target_temporary_attribute_difference_counts ?? {}).length !== 0 ||
    Number(summary?.exact_observed_input_candidate_pairs) !== 0 ||
    Number(summary?.target_current_hp_excluded_candidate_pairs) !== 0 ||
    Number(summary?.strict_controlled_counterfactual_pairs) !== 0 ||
    summary?.exact_damage_projection_proven !== false ||
    summary?.exact_operation_order_proven !== false ||
    summary?.exact_integer_rounding_proven !== false ||
    summary?.packet_conservation_proven !== false || summary?.formula_authority !== false ||
    summary?.runtime_authority !== false || summary?.ui_display_authority !== false ||
    summary?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(proof?.sessions) || proof.sessions.length !== 26 ||
    !Array.isArray(proof?.examples) || proof.examples.length !== 0 ||
    !Array.isArray(proof?.same_context_mismatch_examples) ||
    proof.same_context_mismatch_examples.length !== 32 ||
    proof.same_context_mismatch_examples.some((example) =>
      example.controlled_counterfactual_pair_proven !== false || example.formula_authority !== false) ||
    closest?.rlog !== "runtime-data/logs/monitor-1787003553387.run-0005.rlog" ||
    closest?.session_id !== "monitor-1787003553387.run-0005" ||
    Number(closest?.segment_index) !== 266 || Number(closest?.present_sequence) !== 378384 ||
    Number(closest?.absent_sequence) !== 378486 || Number(closest?.pair_gap_micros) !== 169986 ||
    Number(closest?.source_actor_id) !== 4555 || Number(closest?.source_entity_uuid) !== 80976347776 ||
    Number(closest?.target_actor_id) !== 4711 || Number(closest?.target_entity_uuid) !== 7075070016 ||
    Number(closest?.ability_id) !== 2031105 || Number(closest?.present_amount) !== 308131 ||
    Number(closest?.absent_amount) !== 308131 || closest?.present_normal_value !== null ||
    closest?.absent_normal_value !== null ||
    JSON.stringify(closest?.source_attribute_ids) !== JSON.stringify([474]) ||
    JSON.stringify(closest?.target_attribute_ids) !== JSON.stringify([443, 474, 11310]) ||
    JSON.stringify(closest?.source_temporary_attribute_ids) !== JSON.stringify([]) ||
    JSON.stringify(closest?.target_temporary_attribute_ids) !== JSON.stringify([]) ||
    JSON.stringify(closest?.source_status_effect_ids) !== JSON.stringify([55342, 2207252]) ||
    JSON.stringify(closest?.target_status_effect_ids) !== JSON.stringify([21432, 2203311, 2203521]) ||
    closest?.only_target_current_hp_differs !== false ||
    Number(closest?.residual_observed_state_dimensions_after_443_474_exclusion) !== 6 ||
    Number(closest
      ?.residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion) !== 5 ||
    closest?.source_target_attribute_snapshots_complete !== false ||
    closest?.selected_provider_exact !== true || closest?.segment_status_baseline_complete !== false) {
    throw new Error("effect RLOG transition counterfactual audit is not exact-build fail-closed evidence");
  }
  return {
    status: "transition-adjacent-local-search-no-exact-observed-input-control",
    source_rlog_count: 26,
    canonical_event_count: 6411565,
    data_gap_count: 16181,
    damage_events: 735016,
    damage_events_with_selected_effect_active: 5463,
    opposite_state_recent_comparisons: 47626,
    same_normalized_damage_context_pairs: 37,
    same_context_and_observed_attribute_pairs: 0,
    same_context_and_nonselected_status_pairs: 0,
    same_context_pairs_with_only_target_current_hp_difference: 0,
    same_context_pairs_after_443_474_attribute_exclusion: 0,
    same_context_pairs_after_443_474_and_target_current_hp_exclusion: 1,
    same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses: 0,
    minimum_residual_observed_state_dimensions_after_443_474_exclusion: 6,
    minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion: 5,
    same_context_source_attribute_difference_counts:
      structuredClone(summary.same_context_source_attribute_difference_counts),
    same_context_target_attribute_difference_counts:
      structuredClone(summary.same_context_target_attribute_difference_counts),
    same_context_source_temporary_attribute_difference_counts:
      structuredClone(summary.same_context_source_temporary_attribute_difference_counts),
    same_context_target_temporary_attribute_difference_counts: {},
    same_context_source_status_difference_counts:
      structuredClone(summary.same_context_source_status_difference_counts),
    same_context_target_status_difference_counts:
      structuredClone(summary.same_context_target_status_difference_counts),
    exact_observed_input_candidate_pairs: 0,
    target_current_hp_excluded_candidate_pairs: 0,
    strict_controlled_counterfactual_pairs: 0,
    closest_residual_pair: structuredClone(closest),
    remote_player_packet_dependency: false,
    exact_damage_projection_proven: false,
    exact_operation_order_proven: false,
    exact_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateLuckyPacketComponentProof(proof, build) {
  const selected = (proof?.rows || []).find((row) => Number(row?.damage_attr_id) === 2203110503);
  const attackLuckyRows = (proof?.rows || [])
    .filter((row) => row?.formula_family === "AttackLucky")
    .sort((left, right) => Number(left.ability_id) - Number(right.ability_id));
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "tools/lucky-packet-component-proof.mjs" ||
    String(proof?.game_build) !== build || String(proof?.packet_build) !== build ||
    proof?.content_sha256 !== orderedContentHash(proof) ||
    proof?.policy?.exact_numeric_ids_and_build_are_authoritative !== true ||
    proof?.policy?.packet_amount_equals_lucky_value_is_authoritative_component_identity !== true ||
    proof?.policy
      ?.static_route_and_packet_occurrence_do_not_prove_nonstandard_formula_semantics !== true ||
    proof?.policy
      ?.physical_or_magic_mitigation_route_is_not_inferred_from_damage_script_name !== true ||
    proof?.policy?.unobserved_static_rows_are_preserved !== true ||
    proof?.policy?.packet_absence_is_not_zero !== true ||
    proof?.policy?.unresolved_evidence_is_hidden !== false ||
    proof?.policy?.packet_component_identity_authority !== true ||
    proof?.policy?.damage_formula_authority !== false ||
    proof?.policy?.runtime_attribution_authority !== false ||
    proof?.policy?.ui_display_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    Number(proof?.summary?.ledger_lucky_rows) !== 11 ||
    Number(proof?.summary?.packet_observed_rows) !== 9 ||
    Number(proof?.summary?.unobserved_ledger_rows) !== 2 ||
    Number(proof?.summary?.packet_damage_results) !== 125183 ||
    Number(proof?.summary?.explicit_lucky_value_exact_matches) !== 125183 ||
    proof?.summary?.packet_component_conservation_proven !== true ||
    proof?.summary?.nonstandard_formula_semantics_proven !== false ||
    proof?.summary?.physical_or_magic_mitigation_route_proven !== false ||
    proof?.summary?.formula_authority !== false ||
    proof?.summary?.runtime_attribution_authority !== false ||
    proof?.summary?.ui_display_authority !== false ||
    proof?.summary?.provider_rdps_credit_allowed !== false ||
    !selected || selected?.lookup_key !== "2031105:3" || Number(selected?.ability_id) !== 2031105 ||
    Number(selected?.hit_event_id) !== 3 || selected?.formula_family !== "MAttackLucky" ||
    selected?.formula_signature_id !== "formula-763154feff63c9b9" ||
    selected?.original_ledger_source_state !== "static-route-requires-packet-source" ||
    selected?.original_route_key_resolution_state !==
      "exact-static-route-awaiting-same-build-packet-occurrence" ||
    Number(selected?.static_routes?.length) !== 1 ||
    selected?.static_routes?.[0]?.owner_table !== "BuffTable" ||
    Number(selected?.static_routes?.[0]?.owner_id) !== 2031105 ||
    Number(selected?.packet_damage_results) !== 7762 ||
    Number(selected?.packet_damage_value_shape?.results) !== 7762 ||
    Number(selected?.packet_damage_value_shape?.with_normal_value) !== 0 ||
    Number(selected?.packet_damage_value_shape?.with_lucky_value) !== 7762 ||
    Number(selected?.packet_damage_value_shape?.with_both_values) !== 0 ||
    Number(selected?.packet_damage_value_shape?.amount_matches_lucky_value) !== 7762 ||
    selected?.same_build_packet_occurrence_proven !== true ||
    selected?.packet_component_identity !== "canonical-amount-equals-lucky-value" ||
    selected?.nonstandard_formula_semantics_proven !== false ||
    selected?.physical_defense_dependency_proven !== false ||
    selected?.magic_defense_dependency_proven !== false ||
    selected?.formula_authority !== false || selected?.runtime_attribution_authority !== false ||
    selected?.ui_display_authority !== false || selected?.provider_rdps_credit_allowed !== false ||
    JSON.stringify(attackLuckyRows.map((row) => [
      Number(row.ability_id),
      Number(row.hit_event_id),
      Number(row.packet_damage_results),
    ])) !== JSON.stringify([
      [2031101, 3, 30281],
      [2031103, 3, 35887],
      [2031104, 3, 14684],
      [2031107, 3, 874],
      [2031109, 3, 1692],
      [2031110, 3, 654],
    ]) ||
    JSON.stringify((proof?.unobserved_ledger_rows || []).map((row) => Number(row.damage_attr_id))) !==
      JSON.stringify([2203110603, 2203110803])) {
    throw new Error("same-build Lucky packet component proof is unsafe or incomplete");
  }
  return {
    status: "same-build-lucky-component-observed-nonstandard-formula-semantics-open",
    ledger_lucky_rows: 11,
    packet_observed_rows: 9,
    unobserved_ledger_rows: 2,
    packet_damage_results: 125183,
    explicit_lucky_value_exact_matches: 125183,
    packet_component_conservation_proven: true,
    selected_row: structuredClone(selected),
    attack_lucky_rows: structuredClone(attackLuckyRows),
    unobserved_damage_attr_ids: [2203110603, 2203110803],
    nonstandard_formula_semantics_proven: false,
    physical_or_magic_mitigation_route_proven: false,
    formula_authority: false,
    runtime_attribution_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateMAttackLuckyMitigationDiagnostic(proof, build, luckyPacketComponentProof) {
  const selection = proof?.selection;
  const physical = proof?.axes?.physical_defense?.counters;
  const magic = proof?.axes?.magic_defense?.counters;
  const expectedCounts = { "2031102": 31284, "2031105": 7762, "2031111": 2065 };
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !==
      "rlogs-bpsr-target-mitigation-transform-proof:selected-ability-diagnostic" ||
    String(proof?.game_build) !== build ||
    JSON.stringify((selection?.ability_ids ?? []).map(Number)) !==
      JSON.stringify([2031102, 2031105, 2031111]) ||
    Number(selection?.hit_event_id) !== 3 ||
    Number(selection?.selected_sample_count) !== 41111 ||
    JSON.stringify(selection?.samples_by_ability_id) !== JSON.stringify(expectedCounts) ||
    Number(selection?.samples_by_ability_id?.["2031105"]) !==
      Number(luckyPacketComponentProof?.selected_row?.packet_damage_results) ||
    proof?.policy?.exact_numeric_ability_ids_hit_event_id_and_build_are_authoritative !== true ||
    proof?.policy?.local_or_offline_evidence_only !== true ||
    proof?.policy?.remote_player_only_packets_are_required !== false ||
    proof?.policy?.remote_player_only_packets_are_treated_as_zero !== false ||
    proof?.policy?.remote_player_only_packets_are_synthesized !== false ||
    proof?.policy?.packet_unobservability_does_not_establish_a_complete_status_baseline !== true ||
    proof?.policy?.absent_controlled_pairs_are_not_formula_proof !== true ||
    proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    Number(physical?.samples_with_axis) !== 0 || Number(physical?.controlled_groups) !== 0 ||
    Number(physical?.distinct_axis_pairs) !== 0 ||
    Number(magic?.samples_with_axis) !== 0 || Number(magic?.controlled_groups) !== 0 ||
    Number(magic?.distinct_axis_pairs) !== 0 ||
    proof?.authority?.exact_target_mitigation_formula_proven !== false ||
    proof?.authority?.exact_operation_order_and_integer_rounding_proven !== false ||
    proof?.authority?.complete_status_baseline_proven !== false ||
    proof?.authority?.packet_conservation_proven !== false ||
    proof?.authority?.formula_authority !== false || proof?.authority?.runtime_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("same-build MAttackLucky mitigation diagnostic is unsafe or incomplete");
  }
  return {
    status: "same-build-mattack-lucky-mitigation-axes-unobserved",
    ability_ids: [2031102, 2031105, 2031111],
    hit_event_id: 3,
    selected_sample_count: 41111,
    samples_by_ability_id: expectedCounts,
    physical_defense_axis_samples: 0,
    magic_defense_axis_samples: 0,
    controlled_mitigation_pairs: 0,
    remote_player_packet_dependency: false,
    absent_axes_are_zero_mitigation: false,
    exact_target_mitigation_formula_proven: false,
    exact_operation_order_and_integer_rounding_proven: false,
    complete_status_baseline_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateAttackLuckyMitigationDiagnostic(proof, build, luckyPacketComponentProof) {
  const selection = proof?.selection;
  const physical = proof?.axes?.physical_defense?.counters;
  const magic = proof?.axes?.magic_defense?.counters;
  const expectedIds = [2031101, 2031103, 2031104, 2031107, 2031109, 2031110];
  const expectedCounts = {
    "2031101": 30281,
    "2031103": 35887,
    "2031104": 14684,
    "2031107": 874,
    "2031109": 1692,
    "2031110": 654,
  };
  const luckyAttackRows = luckyPacketComponentProof?.attack_lucky_rows ?? [];
  const luckyAttackPackets = luckyAttackRows.reduce(
    (sum, row) => sum + Number(row?.packet_damage_results ?? 0),
    0,
  );
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !==
      "rlogs-bpsr-target-mitigation-transform-proof:selected-ability-diagnostic" ||
    String(proof?.game_build) !== build ||
    JSON.stringify((selection?.ability_ids ?? []).map(Number)) !== JSON.stringify(expectedIds) ||
    Number(selection?.hit_event_id) !== 3 ||
    Number(selection?.selected_sample_count) !== 84072 ||
    JSON.stringify(selection?.samples_by_ability_id) !== JSON.stringify(expectedCounts) ||
    luckyAttackRows.length !== 6 || luckyAttackPackets !== 84072 ||
    proof?.policy?.exact_numeric_ability_ids_hit_event_id_and_build_are_authoritative !== true ||
    proof?.policy?.local_or_offline_evidence_only !== true ||
    proof?.policy?.remote_player_only_packets_are_required !== false ||
    proof?.policy?.remote_player_only_packets_are_treated_as_zero !== false ||
    proof?.policy?.remote_player_only_packets_are_synthesized !== false ||
    proof?.policy?.packet_unobservability_does_not_establish_a_complete_status_baseline !== true ||
    proof?.policy?.absent_controlled_pairs_are_not_formula_proof !== true ||
    proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    Number(physical?.samples_with_axis) !== 0 || Number(physical?.controlled_groups) !== 0 ||
    Number(physical?.distinct_axis_pairs) !== 0 ||
    Number(magic?.samples_with_axis) !== 0 || Number(magic?.controlled_groups) !== 0 ||
    Number(magic?.distinct_axis_pairs) !== 0 ||
    proof?.authority?.exact_target_mitigation_formula_proven !== false ||
    proof?.authority?.exact_operation_order_and_integer_rounding_proven !== false ||
    proof?.authority?.complete_status_baseline_proven !== false ||
    proof?.authority?.packet_conservation_proven !== false ||
    proof?.authority?.formula_authority !== false || proof?.authority?.runtime_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("same-build AttackLucky mitigation diagnostic is unsafe or incomplete");
  }
  return {
    status: "same-build-attack-lucky-mitigation-axes-unobserved",
    ability_ids: expectedIds,
    hit_event_id: 3,
    selected_sample_count: 84072,
    samples_by_ability_id: expectedCounts,
    physical_defense_axis_samples: 0,
    magic_defense_axis_samples: 0,
    controlled_mitigation_pairs: 0,
    remote_player_packet_dependency: false,
    absent_axes_are_zero_mitigation: false,
    exact_target_mitigation_formula_proven: false,
    exact_operation_order_and_integer_rounding_proven: false,
    complete_status_baseline_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateLuckyParentMultiplierProof(proof, build) {
  const coverage = proof?.coverage;
  const proportional = proof?.formula_tests?.parent_coefficient_proportional;
  const multiplier = proof?.formula_tests?.parent_amount_times_lucky_multiplier;
  const attackMultiplier = proof?.formula_tests?.source_attack_times_lucky_multiplier;
  const magicAttackMultiplier = proof?.formula_tests?.source_magic_attack_times_lucky_multiplier;
  const surface = proof?.inputs?.damage_surface;
  const rlogs = proof?.inputs?.rlogs;
  const descriptorsAreExact = (descriptors) => descriptors.every((descriptor) =>
    String(descriptor?.path ?? "").length > 0 &&
    Number.isSafeInteger(Number(descriptor?.bytes)) && Number(descriptor.bytes) > 0 &&
    /^[0-9a-f]{64}$/.test(String(descriptor?.sha256 ?? ""))
  );
  const parentCount = Object.values(proof?.parent_identity_counts ?? {})
    .reduce((sum, count) => sum + Number(count), 0);
  const relationGroups = proof?.relation_groups;
  const sourceAttributeCandidates = proof?.source_attribute_candidates;
  const groupedEvents = Array.isArray(relationGroups)
    ? relationGroups.reduce((sum, group) => sum + Number(group?.events ?? 0), 0)
    : -1;
  const groupedExactEvents = Array.isArray(relationGroups)
    ? relationGroups.reduce(
      (sum, group) => sum + Number(group?.source_attack_multiplier_exact_events ?? 0),
      0,
    )
    : -1;
  const sourceAttributeCandidatePairs = Array.isArray(sourceAttributeCandidates)
    ? sourceAttributeCandidates.reduce((sum, candidate) => sum + Number(candidate?.events_present ?? 0), 0)
    : -1;
  const sourceAttributeCandidateExactMatches = Array.isArray(sourceAttributeCandidates)
    ? sourceAttributeCandidates.reduce(
      (sum, candidate) =>
        sum + Number(candidate?.floor_attribute_times_lucky_multiplier?.exact_events ?? 0),
      0,
    )
    : -1;
  const sourceAttributeFullCoverageIds = Array.isArray(sourceAttributeCandidates)
    ? sourceAttributeCandidates.filter((candidate) => Number(candidate?.events_present) === 1289).length
    : -1;
  const sourceAttributeVaryingIds = Array.isArray(sourceAttributeCandidates)
    ? sourceAttributeCandidates.filter((candidate) => Number(candidate?.distinct_values) > 1).length
    : -1;
  const sourceAttributeWithinGroupVaryingIds = Array.isArray(sourceAttributeCandidates)
    ? sourceAttributeCandidates.filter(
      (candidate) => Number(candidate?.relation_groups_with_within_group_variation) > 0,
    ).length
    : -1;
  const sourceAttackCandidate = Array.isArray(sourceAttributeCandidates)
    ? sourceAttributeCandidates.find((candidate) => Number(candidate?.attribute_id) === 11330)
    : null;
  if (Number(proof?.schema_version) !== 5 ||
    proof?.generated_by !== "rlogs-bpsr-lucky-strike-formula-proof" ||
    String(proof?.game_build) !== build ||
    proof?.policy?.exact_numeric_build_is_authoritative !== true ||
    proof?.policy?.local_or_offline_evidence_only !== true ||
    proof?.policy?.remote_player_only_packets_are_required !== false ||
    proof?.policy?.remote_player_only_packets_are_treated_as_zero !== false ||
    proof?.policy?.remote_player_only_packets_are_synthesized !== false ||
    proof?.policy?.current_character_snapshot_substitution_allowed !== false ||
    proof?.policy?.rlogs_are_streamed_one_at_a_time !== true ||
    proof?.policy?.wire_group_state_is_bounded_to_one_wire_message !== true ||
    proof?.policy?.examples_are_bounded !== true ||
    proof?.policy?.formula_authority !== false ||
    proof?.policy?.source_attribute_candidate_family_authority !== false ||
    !String(proof?.policy?.local_source_attribute_inventory_scope ?? "")
      .includes("structurally unavailable remote-player-only packets are neither required nor inferred") ||
    proof?.policy?.unresolved_evidence_is_hidden !== false ||
    !surface || !descriptorsAreExact([surface]) ||
    !String(surface.path).replaceAll("\\", "/").endsWith(
      `/steam-${build}/damage-formula-surface.semantic.v1.json`,
    ) ||
    !Array.isArray(rlogs) || rlogs.length !== 26 || !descriptorsAreExact(rlogs) ||
    new Set(rlogs.map((entry) => String(entry.path).toLowerCase())).size !== 26 ||
    Number(coverage?.source_rlog_count) !== 26 || Number(coverage?.wire_messages) !== 35359 ||
    Number(coverage?.lucky_events) !== 1692 || String(coverage?.lucky_observed_damage) !== "59479454" ||
    Number(coverage?.events_with_packet_multiplier) !== 1289 ||
    Number(coverage?.events_with_packet_attack) !== 1289 ||
    Number(coverage?.events_with_packet_magic_attack) !== 0 ||
    Number(coverage?.events_with_one_same_group_parent) !== 1641 ||
    Number(coverage?.events_with_one_adjacent_parent) !== 1687 ||
    Number(coverage?.events_with_one_immediate_following_parent) !== 1692 ||
    Number(coverage?.events_resolved_by_adjacency) !== 51 ||
    Number(coverage?.events_with_ambiguous_parent) !== 0 ||
    Number(coverage?.events_without_parent) !== 0 || parentCount !== 1692 ||
    Number(coverage?.source_attribute_candidate_events) !== 1289 ||
    Number(coverage?.source_attribute_candidate_pairs) !== 177361 ||
    proportional?.expression !==
      "floor(parent_amount * AttrLuckDamInc / selected_parent_PVEDamageRadio)" ||
    Number(proportional?.events) !== 1289 || Number(proportional?.exact_events) !== 0 ||
    String(proportional?.maximum_absolute_residual) !== "310824" ||
    multiplier?.expression !== "floor(parent_amount * AttrLuckDamInc / 10000)" ||
    Number(multiplier?.events) !== 1289 || Number(multiplier?.exact_events) !== 0 ||
    String(multiplier?.maximum_absolute_residual) !== "988255" ||
    attackMultiplier?.expression !== "floor(AttrAttack * AttrLuckDamInc / 10000)" ||
    Number(attackMultiplier?.events) !== 1289 || Number(attackMultiplier?.exact_events) !== 0 ||
    String(attackMultiplier?.minimum_residual) !== "1823" ||
    String(attackMultiplier?.maximum_residual) !== "84858" ||
    magicAttackMultiplier?.expression !== "floor(AttrMAttack * AttrLuckDamInc / 10000)" ||
    Number(magicAttackMultiplier?.events) !== 0 || Number(magicAttackMultiplier?.exact_events) !== 0 ||
    !Array.isArray(relationGroups) || relationGroups.length !== 52 || groupedEvents !== 1289 ||
    groupedExactEvents !== 0 || relationGroups.some((group) =>
      !Number.isSafeInteger(Number(group?.events)) || Number(group.events) <= 0 ||
      Number(group?.source_attack_multiplier_exact_events) !== 0 ||
      Number(group?.source_attack_multiplier_minimum_residual) < 1823 ||
      Number(group?.source_attack_multiplier_maximum_residual) > 84858
    ) ||
    !Array.isArray(sourceAttributeCandidates) || sourceAttributeCandidates.length !== 224 ||
    new Set(sourceAttributeCandidates.map((candidate) => Number(candidate?.attribute_id))).size !== 224 ||
    sourceAttributeCandidates.some((candidate, index) => {
      const attributeId = Number(candidate?.attribute_id);
      const previousAttributeId = index > 0
        ? Number(sourceAttributeCandidates[index - 1]?.attribute_id)
        : -Infinity;
      const eventsPresent = Number(candidate?.events_present);
      const formula = candidate?.floor_attribute_times_lucky_multiplier;
      return !Number.isSafeInteger(attributeId) || attributeId <= previousAttributeId ||
        !Number.isSafeInteger(eventsPresent) || eventsPresent <= 0 || eventsPresent > 1289 ||
        Number(candidate?.missing_candidate_events) !== 1289 - eventsPresent ||
        !Number.isSafeInteger(Number(candidate?.distinct_values)) ||
        Number(candidate.distinct_values) <= 0 || Number(candidate.distinct_values) > eventsPresent ||
        !/^-?\d+$/.test(String(candidate?.minimum_value ?? "")) ||
        !/^-?\d+$/.test(String(candidate?.maximum_value ?? "")) ||
        Number(candidate?.relation_groups_present) <= 0 || Number(candidate.relation_groups_present) > 52 ||
        Number(candidate?.relation_groups_with_within_group_variation) < 0 ||
        Number(candidate.relation_groups_with_within_group_variation) >
          Number(candidate.relation_groups_present) ||
        formula?.expression !== `floor(Attr[${attributeId}] * AttrLuckDamInc / 10000)` ||
        Number(formula?.events) !== eventsPresent || Number(formula?.exact_events) !== 0 ||
        !/^\d+$/.test(String(formula?.absolute_residual_sum ?? "")) ||
        !/^\d+$/.test(String(formula?.maximum_absolute_residual ?? ""));
    }) ||
    sourceAttributeCandidatePairs !== 177361 || sourceAttributeCandidateExactMatches !== 0 ||
    sourceAttributeFullCoverageIds !== 67 || sourceAttributeVaryingIds !== 164 ||
    sourceAttributeWithinGroupVaryingIds !== 163 ||
    Number(sourceAttackCandidate?.events_present) !== 1289 ||
    Number(sourceAttackCandidate?.distinct_values) !== 213 ||
    String(sourceAttackCandidate?.minimum_value) !== "2996" ||
    String(sourceAttackCandidate?.maximum_value) !== "11182" ||
    Number(sourceAttackCandidate?.relation_groups_present) !== 52 ||
    Number(sourceAttackCandidate?.relation_groups_with_within_group_variation) !== 41 ||
    String(sourceAttackCandidate?.floor_attribute_times_lucky_multiplier?.minimum_residual) !== "1823" ||
    String(sourceAttackCandidate?.floor_attribute_times_lucky_multiplier?.maximum_residual) !== "84858" ||
    !Array.isArray(proof?.examples) || proof.examples.length > 24) {
    throw new Error("same-build Lucky parent and multiplier proof is unsafe or incomplete");
  }
  return {
    status: "exact-current-build-lucky-parent-complete-all-local-single-attribute-multiplier-candidates-rejected",
    source_rlog_count: 26,
    lucky_ability_id: 2031109,
    lucky_hit_event_id: 3,
    lucky_events: 1692,
    lucky_observed_damage: "59479454",
    immediate_same_wire_parent_events: 1692,
    unresolved_parent_events: 0,
    ambiguous_parent_events: 0,
    multiplier_candidate_events: 1289,
    multiplier_candidate_exact_matches: 0,
    source_attack_candidate_events: 1289,
    source_attack_candidate_exact_matches: 0,
    source_magic_attack_candidate_events: 0,
    relation_groups: 52,
    source_attack_candidate_minimum_residual: 1823,
    source_attack_candidate_maximum_residual: 84858,
    tested_multiplier_formulas_with_observations_rejected: 3,
    source_attribute_candidate_events: 1289,
    source_attribute_candidate_pairs: 177361,
    source_attribute_candidate_ids: 224,
    source_attribute_full_coverage_ids: 67,
    source_attribute_varying_ids: 164,
    source_attribute_within_relation_group_varying_ids: 163,
    source_attribute_candidate_exact_matches: 0,
    simple_local_single_attribute_candidate_family_exhausted: true,
    remote_player_packet_dependency: false,
    parent_relation_for_observed_subset_proven: true,
    lucky_multiplier_formula_proven: false,
    general_lucky_formula_semantics_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateRlogOpaqueAttributeAudit(proof, build, effectId, gapAuditInput) {
  const summary = proof?.summary;
  const attribute443 = summary?.attributes?.[443];
  const attribute474 = summary?.attributes?.[474];
  const boundGapAudit = proof?.inputs?.gap_window_audit;
  if (Number(proof?.schema_version) !== 2 ||
    proof?.generated_by !== "rlogs-bpsr-rlog-opaque-attribute-audit" ||
    String(proof?.game_build) !== build || Number(proof?.gap_window_effect_id) !== effectId ||
    JSON.stringify(proof?.attribute_ids?.map(Number)) !== JSON.stringify([443, 474]) ||
    proof?.content_sha256 !== `sha256:${orderedContentHash(proof)}` ||
    proof?.policy?.sealed_rlogs_are_streamed_one_event_at_a_time !== true ||
    proof?.policy?.every_data_gap_pause_and_run_boundary_resets_prior_attribute_state !== true ||
    proof?.policy?.wire_adjacency_requires_exact_capture_connection_and_stream_identity !== true ||
    proof?.policy?.generic_varint_interpretation_is_diagnostic_only !== true ||
    proof?.policy?.protobuf_pair_collection_interpretation_is_diagnostic_only !== true ||
    proof?.policy?.opaque_attributes_are_not_excluded_without_semantic_proof !== true ||
    proof?.policy?.packet_absence_is_not_zero !== true ||
    proof?.policy
      ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    proof?.policy?.remote_player_packet_dependency !== false ||
    proof?.policy?.formula_input_semantics_proven !== false ||
    proof?.policy?.damage_consequence_semantics_proven !== false ||
    proof?.policy?.safe_to_exclude_from_counterfactual_matching !== false ||
    proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
    proof?.policy?.ui_display_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    !boundGapAudit || Number(boundGapAudit.bytes) !== Number(gapAuditInput.bytes) ||
    String(boundGapAudit.sha256).replace(/^sha256:/, "") !==
      String(gapAuditInput.sha256).replace(/^sha256:/, "") ||
    Number(summary?.source_rlog_count) !== 26 || Number(summary?.source_rlog_bytes) !== 238642397 ||
    Number(summary?.canonical_event_count) !== 6411565 || Number(summary?.data_gap_count) !== 16181 ||
    Number(summary?.run_boundary_count) !== 66 ||
    summary?.formula_input_semantics_proven !== false ||
    summary?.damage_consequence_semantics_proven !== false ||
    summary?.safe_to_exclude_from_counterfactual_matching !== false ||
    summary?.formula_authority !== false ||
    Number(attribute443?.observation_count) !== 71036 ||
    Number(attribute443?.snapshot_observation_count) !== 832 ||
    Number(attribute443?.delta_observation_count) !== 70204 ||
    Number(attribute443?.canonical_decoder_unresolved_count) !== 71036 ||
    Number(attribute443?.diagnostic_unsigned_varint_valid_count) !== 71036 ||
    Number(attribute443?.diagnostic_unsigned_varint_min) !== 0 ||
    Number(attribute443?.diagnostic_unsigned_varint_max) !== 384000 ||
    Number(attribute443?.diagnostic_signed_prior_delta_counts?.[-22]) !== 3681 ||
    Number(attribute443?.same_wire_related_damage_pairs) !== 399769 ||
    attribute443?.safe_to_exclude_from_counterfactual_matching !== false ||
    Number(attribute474?.observation_count) !== 266216 ||
    Number(attribute474?.snapshot_observation_count) !== 56 ||
    Number(attribute474?.delta_observation_count) !== 266160 ||
    Number(attribute474?.canonical_decoder_unresolved_count) !== 266216 ||
    Number(attribute474?.diagnostic_pair_collection_valid_count) !== 266216 ||
    Number(attribute474?.diagnostic_pair_collection_invalid_count) !== 0 ||
    Number(attribute474?.diagnostic_pair_entry_count) !== 1502529 ||
    Number(attribute474?.diagnostic_pair_entries_with_session_entity_key) !== 1501983 ||
    Number(attribute474?.diagnostic_distinct_pair_key_count) !== 764 ||
    Number(attribute474?.diagnostic_distinct_pair_keys_matching_session_entities) !== 758 ||
    Number(attribute474?.same_wire_related_damage_pairs) !== 1180203 ||
    attribute474?.safe_to_exclude_from_counterfactual_matching !== false ||
    !Array.isArray(proof?.sessions) || proof.sessions.length !== 26 ||
    !Array.isArray(proof?.raw_value_examples) || proof.raw_value_examples.length !== 64 ||
    !Array.isArray(proof?.same_wire_damage_examples) || proof.same_wire_damage_examples.length !== 64 ||
    proof.raw_value_examples.some((example) => example.formula_authority !== false) ||
    proof.same_wire_damage_examples.some((example) => example.formula_authority !== false)) {
    throw new Error("RLOG opaque attribute audit is not exact-build fail-closed evidence");
  }
  return {
    status: "opaque-attributes-443-474-structurally-characterized-semantic-exclusion-unproven",
    source_rlog_count: 26,
    canonical_event_count: 6411565,
    reset_boundary_count: 16247,
    remote_player_packet_dependency: false,
    attribute_443: {
      observation_count: 71036,
      scalar_shape_observation_count: 71036,
      scalar_min: 0,
      scalar_max: 384000,
      most_common_signed_prior_delta: -22,
      most_common_signed_prior_delta_count: 3681,
      same_wire_related_damage_pairs: 399769,
      semantic_identity_proven: false,
      safe_to_exclude_from_counterfactual_matching: false,
    },
    attribute_474: {
      observation_count: 266216,
      pair_collection_shape_observation_count: 266216,
      pair_entry_count: 1502529,
      pair_entries_matching_session_entities: 1501983,
      distinct_pair_key_count: 764,
      distinct_pair_keys_matching_session_entities: 758,
      same_wire_related_damage_pairs: 1180203,
      semantic_identity_proven: false,
      safe_to_exclude_from_counterfactual_matching: false,
    },
    formula_input_semantics_proven: false,
    damage_consequence_semantics_proven: false,
    safe_to_exclude_from_counterfactual_matching: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateSourceStatusConfounderRouteAudit(proof, build) {
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "tools/bpsr-source-status-confounder-route-audit.mjs" ||
    String(proof?.game_build) !== build || Number(proof?.effect_id) !== 55342 ||
    Number(proof?.linked_action_id) !== 25534201 ||
    proof?.content_sha256 !== orderedContentHash(proof) ||
    proof?.status !== "healing-action-route-proven-status-damage-neutrality-unproven" ||
    proof?.policy?.remote_player_packet_acquisition_required !== false ||
    proof?.policy?.never_received_remote_player_packet_absence_is_not_zero !== true ||
    proof?.policy?.produced_action_healing_only_does_not_prove_status_modifier_damage_neutrality !== true ||
    Number(proof?.packet_observed_action_outcomes?.packet_damage_results) !== 0 ||
    Number(proof?.packet_observed_action_outcomes?.packet_healing_results) !== 22320 ||
    proof?.packet_observed_action_outcomes?.status_modifier_damage_neutrality_proven !== false ||
    Number(proof?.counterfactual_confounder?.same_normalized_damage_context_pairs) !== 37 ||
    Number(proof?.counterfactual_confounder?.same_context_source_status_difference_count) !== 33 ||
    Number(proof?.counterfactual_confounder?.strict_controlled_counterfactual_pairs) !== 0 ||
    proof?.counterfactual_confounder?.may_exclude_from_counterfactual_matching !== false ||
    Number(proof?.static_formula_coverage?.direct_exact_effect_token_matches) !== 0 ||
    proof?.static_formula_coverage?.absence_proves_damage_neutrality !== false ||
    proof?.conclusion?.structural_remote_player_packets_required_to_close !== false ||
    proof?.authority?.formula_authority !== false || proof?.authority?.runtime_authority !== false ||
    proof?.authority?.ui_display_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("source-status 55342 confounder route audit is not exact-build fail-closed evidence");
  }
  return {
    status: "healing-action-route-proven-status-damage-neutrality-unproven",
    effect_id: 55342,
    linked_action_id: 25534201,
    packet_damage_results: 0,
    packet_healing_results: 22320,
    same_context_source_status_difference_count: 33,
    strict_controlled_counterfactual_pairs: 0,
    remote_player_packet_acquisition_required: false,
    status_modifier_damage_neutrality_proven: false,
    may_exclude_from_counterfactual_matching: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateSourceStatusLocalObservableAudit(proof, build) {
  const transfer = proof?.effect_2207252;
  const delta = transfer?.locally_observed_dynamic_recipient_delta_subset?.attributes ?? {};
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "tools/bpsr-source-status-local-observable-audit.mjs" ||
    String(proof?.game_build) !== build || proof?.content_sha256 !== orderedContentHash(proof) ||
    proof?.policy
      ?.structurally_unavailable_remote_player_attributes_are_not_acquisition_requirements !== true ||
    proof?.policy?.unavailable_provider_attribute_absence_is_not_zero !== true ||
    proof?.policy
      ?.current_character_snapshots_never_substitute_for_historical_provider_attributes !== true ||
    Number(proof?.effect_701010?.packet_lifecycle?.window_count) !== 62935 ||
    Number(proof?.effect_701010?.reconciliation?.unresolved_cross_actor_windows) !== 62935 ||
    proof?.effect_701010?.provider_identity_proven !== false ||
    proof?.effect_701010?.safe_to_exclude_from_counterfactual_matching !== false ||
    Number(transfer?.reconciliation?.resolved_external_player_to_player_windows) !== 12948 ||
    Number(transfer?.reconciliation?.unresolved_cross_actor_windows) !== 0 ||
    transfer?.reconciliation?.exact_owning_source !== true ||
    Number(delta[11030]?.occurrences) !== 48 || Number(delta[11030]?.applications) !== 22 ||
    Number(delta[11030]?.removals) !== 26 ||
    Number(delta[11030]?.independent_run_contexts) !== 16 ||
    Number(delta[11030]?.provider_attribute_context_examples) !== 0 ||
    Number(delta[11033]?.occurrences) !== 47 || Number(delta[11330]?.occurrences) !== 49 ||
    Number(delta[11331]?.occurrences) !== 49 || Number(delta[11332]?.occurrences) !== 48 ||
    transfer?.provider_attribute_context?.remote_player_attribute_acquisition_required !== false ||
    transfer?.provider_attribute_context?.current_snapshot_substitution_allowed !== false ||
    transfer?.general_formula?.percent_magnitude_proven_from_exact_join !== false ||
    transfer?.general_formula?.integer_rounding_proven !== false ||
    transfer?.general_formula?.exact_damage_projection_proven !== false ||
    transfer?.full_lifecycle_formula_replayable !== false ||
    transfer?.safe_to_exclude_from_counterfactual_matching !== false ||
    Number(proof?.transition_confounder?.source_status_difference_counts?.[701010]) !== 29 ||
    Number(proof?.transition_confounder?.source_status_difference_counts?.[2207252]) !== 29 ||
    proof?.transition_confounder?.both_effects_remain_confounders !== true ||
    proof?.authority?.formula_authority !== false || proof?.authority?.runtime_authority !== false ||
    proof?.authority?.ui_display_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("source-status local-observable audit is not exact-build fail-closed evidence");
  }
  return {
    status: "local-external-stat-transfer-subset-observed-general-formula-unproven",
    effect_701010_windows: 62935,
    effect_701010_unresolved_cross_actor_windows: 62935,
    effect_2207252_external_player_windows: 12948,
    effect_2207252_exact_agility_delta_occurrences: 48,
    effect_2207252_exact_agility_delta_independent_runs: 16,
    remote_provider_attribute_context_examples: 0,
    remote_player_attribute_acquisition_required: false,
    current_snapshot_substitution_allowed: false,
    general_transfer_percent_proven: false,
    integer_rounding_proven: false,
    exact_damage_projection_proven: false,
    both_effects_remain_counterfactual_confounders: true,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyReport(report) {
  const expectedHash = contentHash(report);
  const schemaVersion = Number(report?.schema_version);
  const exactOwnershipRequired = schemaVersion >= 8;
  const unresolvedProviderEvents = Number(report?.summary?.unresolved_provider_status_events);
  if (!SUPPORTED_SCHEMA_VERSIONS.has(Number(report?.schema_version)) || report?.generated_by !== GENERATOR ||
    !/^\d+$/.test(String(report?.game_build ?? "")) || Number(report?.effect_id) !== 2110092 ||
    report?.content_sha256 !== expectedHash ||
    report?.policy?.exact_numeric_ids_and_build_are_authoritative !== true ||
    report?.policy?.exact_input_hashes_are_embedded !== true ||
    report?.policy?.formula_authority !== false || report?.policy?.runtime_authority !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    report?.build_identity?.exhaustive_source_manifest_complete !== true ||
    report?.build_identity?.exact_static_table_hash_binding_proven !== true ||
    report?.build_identity?.decoded_table_bindings?.length !== 4 ||
    report?.static_scalar?.exact_static_scalar_proven !== true ||
    report?.static_scalar?.ladders_exactly_equal !== true ||
    Number(report?.summary?.observed_runtime_tier) !== 5 ||
    Number(report?.summary?.observed_runtime_armor_penetration_basis_points) !== 650 ||
    Number(report?.summary?.observed_runtime_armor_penetration_percent) !== 6.5 ||
    (exactOwnershipRequired
      ? (unresolvedProviderEvents !== 0 || report?.summary?.exact_provider_ownership_proven !== true)
      : (unresolvedProviderEvents <= 0 || report?.summary?.exact_provider_ownership_proven !== false)) ||
    report?.summary?.exact_damage_projection_proven !== false ||
    report?.summary?.packet_conservation_proven !== false ||
    report?.summary?.formula_authority !== false || report?.summary?.runtime_authority !== false ||
    report?.summary?.provider_rdps_credit_allowed !== false ||
    report?.counterfactual_projection?.exact_armor_to_damage_equation_proven !== false ||
    report?.counterfactual_projection?.exact_operation_order_proven !== false ||
    report?.counterfactual_projection?.exact_integer_rounding_proven !== false ||
    report?.counterfactual_projection?.packet_conservation_proven !== false) {
    throw new Error("Blade Sweep scalar proof violates its schema or fail-closed authority contract");
  }
  if (Number(report.schema_version) >= 2 &&
    ((exactOwnershipRequired
      ? report?.provider_ownership_gap_worklist?.status !== "exact-provider-ownership-proven"
      : report?.provider_ownership_gap_worklist?.status !== "exact-gap-inventory-acquisition-required") ||
      Number(report?.provider_ownership_gap_worklist?.unresolved_status_events) !==
        Number(report?.summary?.unresolved_provider_status_events) ||
      (exactOwnershipRequired
        ? (Number(report?.provider_ownership_gap_worklist?.gap_groups) !== 0 ||
          report?.provider_ownership_gap_worklist?.exact_provider_ownership_proven !== true)
        : (Number(report?.provider_ownership_gap_worklist?.gap_groups) <= 0 ||
          report?.provider_ownership_gap_worklist?.exact_provider_ownership_proven !== false)) ||
      report?.provider_ownership_gap_worklist?.formula_authority !== false ||
      report?.provider_ownership_gap_worklist?.runtime_authority !== false ||
      report?.provider_ownership_gap_worklist?.provider_rdps_credit_allowed !== false)) {
    throw new Error("Blade Sweep scalar proof has an unsafe provider ownership gap worklist");
  }
  if (Number(report.schema_version) >= 7 &&
    (!Number.isSafeInteger(Number(
      report?.provider_ownership?.events_with_prior_status_instance_player_owner,
    )) ||
      Number(report.provider_ownership.events_with_prior_status_instance_player_owner) <= 0 ||
      Number(report?.provider_ownership_gap_worklist
        ?.prior_status_instance_player_owned_status_events) !==
        Number(report.provider_ownership.events_with_prior_status_instance_player_owner))) {
    throw new Error("Blade Sweep scalar proof has unsafe forward status-instance ownership evidence");
  }
  if (Number(report.schema_version) >= 8 &&
    (!Number.isSafeInteger(Number(
      report?.provider_ownership?.events_with_same_wire_packet_player_owner,
    )) ||
      Number(report.provider_ownership.events_with_same_wire_packet_player_owner) <= 0 ||
      Number(report?.provider_ownership_gap_worklist
        ?.same_wire_packet_player_owned_status_events) !==
        Number(report.provider_ownership.events_with_same_wire_packet_player_owner) ||
      report?.provider_ownership?.exact_provider_ownership_for_every_event_proven !== true)) {
    throw new Error("Blade Sweep scalar proof has unsafe same-wire-packet ownership evidence");
  }
  if (Number(report.schema_version) >= 3 &&
    (report?.policy?.aggregate_offline_exhaustion_is_not_combat_formula_proof !== true ||
      report?.target_mitigation_offline_exhaustion?.status !==
        "exact-current-build-aggregate-offline-client-and-packet-search-exhausted-final-validation-required" ||
      Number(report?.target_mitigation_offline_exhaustion?.packet_capture_proofs) <= 0 ||
      Number(report?.target_mitigation_offline_exhaustion?.packet_source_rlogs) <
        Number(report?.target_mitigation_offline_exhaustion?.packet_capture_proofs) ||
      Number(report?.target_mitigation_offline_exhaustion?.packet_damage_samples) <
        Number(report?.global_target_mitigation_evidence?.damage_samples) ||
      Number(report?.target_mitigation_offline_exhaustion?.packet_audited_axis_samples) <
        Number(report?.global_target_mitigation_evidence?.audited_axis_samples) ||
      Number(report?.target_mitigation_offline_exhaustion?.packet_samples_with_physical_or_refined_defense) <= 0 ||
      Number(report?.target_mitigation_offline_exhaustion?.controlled_counterfactual_pairs) !== 0 ||
      Number(report?.target_mitigation_offline_exhaustion?.promoted_combat_formulas) !== 0 ||
      !Array.isArray(report?.target_mitigation_offline_exhaustion?.final_validation) ||
      report.target_mitigation_offline_exhaustion.final_validation.length !== 2 ||
      report?.target_mitigation_offline_exhaustion?.exact_target_mitigation_formula_proven !== false ||
      report?.target_mitigation_offline_exhaustion?.operation_order_and_integer_rounding_proven !== false ||
      report?.target_mitigation_offline_exhaustion?.packet_conservation_proven !== false ||
      report?.target_mitigation_offline_exhaustion?.formula_authority !== false ||
      report?.target_mitigation_offline_exhaustion?.runtime_authority !== false ||
      report?.target_mitigation_offline_exhaustion?.provider_rdps_credit_allowed !== false)) {
    throw new Error("Blade Sweep scalar proof has unsafe offline target mitigation exhaustion evidence");
  }
  if (Number(report.schema_version) >= 4 &&
    (report?.policy?.target_status_relaxed_near_pairs_are_not_combat_formula_proof !== true ||
      report?.target_mitigation_acquisition_worklist?.status !==
        (Number(report.schema_version) >= 9
          ? "acquisition-required-strict-controls-status-damage-relevance-observed"
          : "acquisition-required-no-target-status-only-near-pair") ||
      Number(report?.target_mitigation_acquisition_worklist?.matching_build_capture_diagnostics) <= 0 ||
      Number(report?.target_mitigation_acquisition_worklist?.damage_samples) !==
        Number(report?.target_mitigation_evidence?.damage_samples) ||
      Number(report?.target_mitigation_acquisition_worklist?.audited_axis_samples) !==
        Number(report?.target_mitigation_evidence?.audited_axis_samples) ||
      Number(report?.target_mitigation_acquisition_worklist?.strict_controlled_groups) !== 0 ||
      Number(report?.target_mitigation_acquisition_worklist
        ?.target_status_relaxed_distinct_axis_pairs) !== 0 ||
      Number(report?.target_mitigation_acquisition_worklist
        ?.pairs_with_effect_in_target_status_delta) !== 0 ||
      !Array.isArray(report?.target_mitigation_acquisition_worklist
        ?.acquisition_contract?.required_controls) ||
      report.target_mitigation_acquisition_worklist.acquisition_contract.required_controls.length === 0 ||
      report?.target_mitigation_acquisition_worklist?.exact_target_mitigation_formula_proven !== false ||
      report?.target_mitigation_acquisition_worklist
        ?.operation_order_and_integer_rounding_proven !== false ||
      report?.target_mitigation_acquisition_worklist?.packet_conservation_proven !== false ||
      report?.target_mitigation_acquisition_worklist?.formula_authority !== false ||
      report?.target_mitigation_acquisition_worklist?.runtime_authority !== false ||
      report?.target_mitigation_acquisition_worklist?.provider_rdps_credit_allowed !== false)) {
    throw new Error("Blade Sweep scalar proof has an unsafe target mitigation acquisition worklist");
  }
  if (Number(report.schema_version) >= 5 &&
    (report?.policy?.status_confounded_integer_candidate_compatibility_is_not_combat_formula_proof !== true ||
      report?.target_mitigation_near_pair_candidate?.status !==
        "exact-integer-candidate-compatible-status-confounded" ||
      report?.target_mitigation_near_pair_candidate?.model_id !==
        "target-physical-armor-counterfactual" ||
      Number(report?.target_mitigation_near_pair_candidate?.transformed_curve_constant) !== 22000 ||
      Number(report?.target_mitigation_near_pair_candidate?.runtime_simple_curve_constant) !== 6500 ||
      Number(report?.target_mitigation_near_pair_candidate?.packet_near_pair_rows) !== 3 ||
      Number(report?.target_mitigation_near_pair_candidate?.transformed_curve_compatible_rows) !== 3 ||
      JSON.stringify(report?.target_mitigation_near_pair_candidate
        ?.transformed_curve_unique_shared_base_values) !== JSON.stringify(["107006"]) ||
      Number(report?.target_mitigation_near_pair_candidate?.runtime_simple_curve_compatible_rows) !== 0 ||
      report?.target_mitigation_near_pair_candidate
        ?.selected_blade_sweep_effect_2110092_in_status_delta !== false ||
      report?.target_mitigation_near_pair_candidate?.exact_status_state_equal !== false ||
      report?.target_mitigation_near_pair_candidate
        ?.effect_2201452_damage_stage_exclusivity_proven !== false ||
      report?.target_mitigation_near_pair_candidate?.exact_target_mitigation_formula_proven !== false ||
      report?.target_mitigation_near_pair_candidate
        ?.operation_order_and_integer_rounding_proven !== false ||
      report?.target_mitigation_near_pair_candidate?.packet_conservation_proven !== false ||
      report?.target_mitigation_near_pair_candidate?.formula_authority !== false ||
      report?.target_mitigation_near_pair_candidate?.runtime_authority !== false ||
      report?.target_mitigation_near_pair_candidate?.provider_rdps_credit_allowed !== false)) {
    throw new Error("Blade Sweep scalar proof has unsafe target mitigation near-pair evidence");
  }
  if (Number(report.schema_version) >= 6 &&
    (Number(report?.target_mitigation_near_pair_candidate
      ?.confounder_counterfactual_exhaustion?.matching_build_capture_proofs) !== 24 ||
      Number(report?.target_mitigation_near_pair_candidate
        ?.confounder_counterfactual_exhaustion?.matching_build_source_rlogs) !== 26 ||
      Number(report?.target_mitigation_near_pair_candidate
        ?.confounder_counterfactual_exhaustion?.damage_samples) !== 735016 ||
      Number(report?.target_mitigation_near_pair_candidate
        ?.confounder_counterfactual_exhaustion?.target_locus_observed_samples) !== 3009 ||
      Number(report?.target_mitigation_near_pair_candidate
        ?.confounder_counterfactual_exhaustion?.exact_target_locus_controlled_groups) !== 0 ||
      report?.target_mitigation_near_pair_candidate?.confounder_counterfactual_exhaustion
        ?.every_common_confounder_observed_at_target_locus !== true ||
      report?.target_mitigation_near_pair_candidate?.confounder_counterfactual_exhaustion
        ?.every_common_confounder_exactly_controlled_at_target_locus !== false ||
      report?.target_mitigation_near_pair_candidate?.confounder_counterfactual_exhaustion
        ?.common_status_confounders_eliminated !== false)) {
    throw new Error("Blade Sweep scalar proof has unsafe status-confounder exhaustion evidence");
  }
  if (Number(report.schema_version) >= 9 &&
    (report?.policy?.structurally_unobservable_remote_player_packets_are_not_formula_acquisition_requirements !== true ||
      report?.target_mitigation_acquisition_worklist
        ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
      Number(report?.target_mitigation_acquisition_worklist?.global_same_axis_target_status_pairs) !== 5 ||
      Number(report?.target_mitigation_acquisition_worklist?.global_same_axis_equal_output_pairs) !== 4 ||
      Number(report?.target_mitigation_acquisition_worklist?.global_same_axis_divergent_output_pairs) !== 1 ||
      Number(report?.target_mitigation_near_pair_candidate?.same_axis_status_invariance
        ?.physical_defense_same_axis_status_pairs) !== 5 ||
      Number(report?.target_mitigation_near_pair_candidate?.same_axis_status_invariance
        ?.physical_defense_same_axis_equal_output_pairs) !== 4 ||
      Number(report?.target_mitigation_near_pair_candidate?.same_axis_status_invariance
        ?.physical_defense_same_axis_divergent_output_pairs) !== 1 ||
      report?.target_mitigation_near_pair_candidate?.same_axis_status_invariance
        ?.target_status_can_change_damage_outside_raw_defense !== true ||
      JSON.stringify(report?.target_mitigation_near_pair_candidate?.same_axis_status_invariance
        ?.candidate_status_effect_ids_without_same_axis_witness) !== JSON.stringify([55301, 2201452]) ||
      Number(report?.summary?.same_axis_target_status_pairs) !== 5 ||
      Number(report?.summary?.same_axis_equal_output_pairs) !== 4 ||
      Number(report?.summary?.same_axis_divergent_output_pairs) !== 1)) {
    throw new Error("Blade Sweep scalar proof has unsafe same-axis target-status evidence");
  }
  if (Number(report.schema_version) >= 10 &&
    (report?.policy?.candidate_counterfactual_discriminants_never_grant_formula_or_ui_authority !== true ||
      report?.counterfactual_discriminants?.status !==
        "exact-candidate-discriminants-awaiting-controlled-packet-proof" ||
      Number(report?.counterfactual_discriminants?.armor_penetration_basis_points) !== 650 ||
      Number(report?.counterfactual_discriminants?.defense_curve_constant) !== 22000 ||
      !Array.isArray(report?.counterfactual_discriminants?.exact_discriminant_rows) ||
      report.counterfactual_discriminants.exact_discriminant_rows.length !== 2 ||
      JSON.stringify(report?.counterfactual_discriminants
        ?.distinct_predicted_damage_with_effect) !== JSON.stringify([85530, 85533, 87122, 87125]) ||
      report?.counterfactual_discriminants?.acquisition_contract
        ?.remote_player_packet_dependency !== false ||
      report?.counterfactual_discriminants?.exact_damage_projection_proven !== false ||
      report?.counterfactual_discriminants?.exact_operation_order_proven !== false ||
      report?.counterfactual_discriminants?.exact_integer_rounding_proven !== false ||
      report?.counterfactual_discriminants?.packet_conservation_proven !== false ||
      report?.counterfactual_discriminants?.formula_authority !== false ||
      report?.counterfactual_discriminants?.runtime_authority !== false ||
      report?.counterfactual_discriminants?.ui_display_authority !== false ||
      report?.counterfactual_discriminants?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.candidate_counterfactual_discriminant_rows) !== 2 ||
      Number(report?.summary?.candidate_counterfactual_distinct_output_signatures) !== 4)) {
    throw new Error("Blade Sweep scalar proof has unsafe counterfactual discriminants");
  }
  if (Number(report.schema_version) >= 25 &&
    (report?.policy
      ?.exact_packet_component_and_coefficient_identity_do_not_prove_defense_stage_order !== true ||
      report?.counterfactual_discriminants?.packet_formula_identity?.status !==
        "exact-build-packet-occurrence-static-route-and-coefficient-bound" ||
      Number(report?.counterfactual_discriminants?.packet_formula_identity?.ability_id) !== 823225 ||
      Number(report?.counterfactual_discriminants?.packet_formula_identity?.hit_event_id) !== 3 ||
      Number(report?.counterfactual_discriminants?.packet_formula_identity?.damage_attr_id) !== 282322503 ||
      JSON.stringify(report?.counterfactual_discriminants?.packet_formula_identity
        ?.pve_damage_ratio_basis_points) !== JSON.stringify([25000]) ||
      Number(report?.counterfactual_discriminants?.packet_formula_identity?.packet_damage_results) !== 185 ||
      report?.counterfactual_discriminants?.packet_formula_identity
        ?.coefficient_to_pre_mitigation_base_formula_proven !== false ||
      report?.counterfactual_discriminants?.observed_baseline_curve?.status !==
        "three-distinct-defense-points-share-exact-integer-base-status-control-absent" ||
      Number(report?.counterfactual_discriminants?.observed_baseline_curve
        ?.exact_curve_compatible_rows) !== 22 ||
      Number(report?.counterfactual_discriminants?.observed_baseline_curve
        ?.preserved_status_confounded_rows) !== 1 ||
      Number(report?.counterfactual_discriminants?.observed_baseline_curve
        ?.unique_shared_nonnegative_base) !== 107006 ||
      report?.counterfactual_discriminants?.observed_baseline_curve
        ?.target_status_control_proven !== false ||
      Number(report?.summary?.exact_packet_damage_component_id) !== 282322503 ||
      Number(report?.summary?.exact_packet_component_coefficient_basis_points) !== 25000 ||
      Number(report?.summary?.actor_scene_curve_compatible_rows) !== 22 ||
      Number(report?.summary?.actor_scene_curve_distinct_defense_points) !== 3 ||
      Number(report?.summary?.actor_scene_curve_status_confounded_rows) !== 1)) {
    throw new Error("Blade Sweep scalar proof has an unsafe packet-formula identity receipt");
  }
  if (Number(report.schema_version) >= 26 &&
    (report?.policy
      ?.same_input_status_invariance_does_not_remove_common_target_status_confounders !== true ||
      Number(report?.counterfactual_discriminants?.observed_baseline_curve
        ?.same_input_status_invariance?.compatible_target_status_state_ids) !== 20 ||
      Number(report?.counterfactual_discriminants?.observed_baseline_curve
        ?.same_input_status_invariance?.common_effect_ids_across_all_compatible_rows?.length) !== 78 ||
      Number(report?.counterfactual_discriminants?.observed_baseline_curve
        ?.same_input_status_invariance?.varying_effect_ids_across_all_compatible_rows?.length) !== 36 ||
      Number(report?.counterfactual_discriminants?.observed_baseline_curve
        ?.same_input_status_invariance?.isolated_single_effect_toggle_count) !== 1 ||
      Number(report?.counterfactual_discriminants?.observed_baseline_curve
        ?.same_input_status_invariance?.same_input_groups?.[1]
        ?.isolated_single_effect_toggle_receipts?.[0]?.effect_id) !== 2203182 ||
      report?.counterfactual_discriminants?.observed_baseline_curve
        ?.same_input_status_invariance?.common_target_status_confounders_remain !== true ||
      report?.counterfactual_discriminants?.observed_baseline_curve
        ?.same_input_status_invariance?.target_status_control_proven !== false ||
      Number(report?.summary?.actor_scene_compatible_target_status_states) !== 20 ||
      Number(report?.summary?.actor_scene_common_target_status_confounders) !== 78 ||
      Number(report?.summary?.actor_scene_varying_target_status_effects) !== 36 ||
      Number(report?.summary?.actor_scene_isolated_invariant_single_effect_toggles) !== 1)) {
    throw new Error("Blade Sweep scalar proof has unsafe same-input status-invariance evidence");
  }
  if (Number(report.schema_version) >= 27 &&
    (report?.policy
      ?.attribute_443_474_and_target_current_hp_exclusion_is_diagnostic_only !== true ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.same_context_pairs_after_443_474_attribute_exclusion) !== 0 ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.same_context_pairs_after_443_474_and_target_current_hp_exclusion) !== 1 ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses) !== 0 ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.minimum_residual_observed_state_dimensions_after_443_474_exclusion) !== 6 ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion) !== 5 ||
      Number(report?.rlog_transition_counterfactual_audit?.closest_residual_pair?.ability_id) !== 2031105 ||
      Number(report?.rlog_transition_counterfactual_audit?.closest_residual_pair?.present_amount) !== 308131 ||
      Number(report?.rlog_transition_counterfactual_audit?.closest_residual_pair?.absent_amount) !== 308131 ||
      JSON.stringify(report?.rlog_transition_counterfactual_audit
        ?.closest_residual_pair?.source_status_effect_ids) !== JSON.stringify([55342, 2207252]) ||
      JSON.stringify(report?.rlog_transition_counterfactual_audit
        ?.closest_residual_pair?.target_status_effect_ids) !== JSON.stringify([21432, 2203311, 2203521]) ||
      report?.rlog_transition_counterfactual_audit
        ?.closest_residual_pair?.source_target_attribute_snapshots_complete !== false ||
      report?.rlog_transition_counterfactual_audit?.closest_residual_pair?.selected_provider_exact !== true ||
      report?.rlog_transition_counterfactual_audit
        ?.closest_residual_pair?.segment_status_baseline_complete !== false ||
      report?.rlog_transition_counterfactual_audit
        ?.closest_residual_pair?.controlled_counterfactual_pair_proven !== false ||
      report?.rlog_transition_counterfactual_audit?.closest_residual_pair?.formula_authority !== false ||
      Number(report?.summary?.effect_2110092_transition_pairs_after_443_474_exclusion) !== 0 ||
      Number(report?.summary
        ?.effect_2110092_transition_pairs_after_443_474_and_target_current_hp_exclusion) !== 1 ||
      Number(report?.summary
        ?.effect_2110092_transition_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses) !== 0 ||
      Number(report?.summary
        ?.effect_2110092_transition_minimum_residual_dimensions_after_diagnostic_exclusions) !== 5)) {
    throw new Error("Blade Sweep scalar proof has unsafe staged transition-exclusion evidence");
  }
  if (Number(report.schema_version) >= 28 &&
    (report?.policy
      ?.lucky_packet_component_identity_does_not_prove_defense_dependency_or_formula_semantics !== true ||
      report?.lucky_packet_component_proof?.status !==
        "same-build-lucky-component-observed-nonstandard-formula-semantics-open" ||
      Number(report?.lucky_packet_component_proof?.ledger_lucky_rows) !== 11 ||
      Number(report?.lucky_packet_component_proof?.packet_observed_rows) !== 9 ||
      Number(report?.lucky_packet_component_proof?.unobserved_ledger_rows) !== 2 ||
      Number(report?.lucky_packet_component_proof?.packet_damage_results) !== 125183 ||
      Number(report?.lucky_packet_component_proof?.explicit_lucky_value_exact_matches) !== 125183 ||
      report?.lucky_packet_component_proof?.packet_component_conservation_proven !== true ||
      Number(report?.lucky_packet_component_proof?.selected_row?.damage_attr_id) !== 2203110503 ||
      report?.lucky_packet_component_proof?.selected_row?.lookup_key !== "2031105:3" ||
      report?.lucky_packet_component_proof?.selected_row?.formula_family !== "MAttackLucky" ||
      Number(report?.lucky_packet_component_proof?.selected_row?.packet_damage_results) !== 7762 ||
      report?.lucky_packet_component_proof?.selected_row?.same_build_packet_occurrence_proven !== true ||
      report?.lucky_packet_component_proof?.selected_row?.packet_component_identity !==
        "canonical-amount-equals-lucky-value" ||
      report?.lucky_packet_component_proof?.selected_row?.nonstandard_formula_semantics_proven !== false ||
      report?.lucky_packet_component_proof?.selected_row?.physical_defense_dependency_proven !== false ||
      report?.lucky_packet_component_proof?.selected_row?.magic_defense_dependency_proven !== false ||
      report?.lucky_packet_component_proof?.nonstandard_formula_semantics_proven !== false ||
      report?.lucky_packet_component_proof?.physical_or_magic_mitigation_route_proven !== false ||
      report?.lucky_packet_component_proof?.formula_authority !== false ||
      report?.lucky_packet_component_proof?.runtime_attribution_authority !== false ||
      report?.lucky_packet_component_proof?.ui_display_authority !== false ||
      report?.lucky_packet_component_proof?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.closest_transition_lucky_damage_attr_id) !== 2203110503 ||
      Number(report?.summary?.closest_transition_lucky_packet_damage_results) !== 7762 ||
      Number(report?.summary?.lucky_packet_component_damage_results) !== 125183 ||
      Number(report?.summary?.lucky_packet_component_exact_matches) !== 125183)) {
    throw new Error("Blade Sweep scalar proof has unsafe Lucky packet component evidence");
  }
  if (Number(report.schema_version) >= 29 &&
    (report?.policy?.absent_observed_mitigation_axes_are_not_zero_mitigation_or_formula_proof !== true ||
      report?.mattack_lucky_mitigation_diagnostic?.status !==
        "same-build-mattack-lucky-mitigation-axes-unobserved" ||
      JSON.stringify(report?.mattack_lucky_mitigation_diagnostic?.ability_ids) !==
        JSON.stringify([2031102, 2031105, 2031111]) ||
      Number(report?.mattack_lucky_mitigation_diagnostic?.hit_event_id) !== 3 ||
      Number(report?.mattack_lucky_mitigation_diagnostic?.selected_sample_count) !== 41111 ||
      Number(report?.mattack_lucky_mitigation_diagnostic?.samples_by_ability_id?.["2031105"]) !==
        Number(report?.lucky_packet_component_proof?.selected_row?.packet_damage_results) ||
      Number(report?.mattack_lucky_mitigation_diagnostic?.physical_defense_axis_samples) !== 0 ||
      Number(report?.mattack_lucky_mitigation_diagnostic?.magic_defense_axis_samples) !== 0 ||
      Number(report?.mattack_lucky_mitigation_diagnostic?.controlled_mitigation_pairs) !== 0 ||
      report?.mattack_lucky_mitigation_diagnostic?.remote_player_packet_dependency !== false ||
      report?.mattack_lucky_mitigation_diagnostic?.absent_axes_are_zero_mitigation !== false ||
      report?.mattack_lucky_mitigation_diagnostic?.exact_target_mitigation_formula_proven !== false ||
      report?.mattack_lucky_mitigation_diagnostic
        ?.exact_operation_order_and_integer_rounding_proven !== false ||
      report?.mattack_lucky_mitigation_diagnostic?.complete_status_baseline_proven !== false ||
      report?.mattack_lucky_mitigation_diagnostic?.packet_conservation_proven !== false ||
      report?.mattack_lucky_mitigation_diagnostic?.formula_authority !== false ||
      report?.mattack_lucky_mitigation_diagnostic?.runtime_authority !== false ||
      report?.mattack_lucky_mitigation_diagnostic?.ui_display_authority !== false ||
      report?.mattack_lucky_mitigation_diagnostic?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.mattack_lucky_selected_damage_results) !== 41111 ||
      Number(report?.summary?.mattack_lucky_physical_defense_axis_samples) !== 0 ||
      Number(report?.summary?.mattack_lucky_magic_defense_axis_samples) !== 0)) {
    throw new Error("Blade Sweep scalar proof has unsafe MAttackLucky mitigation evidence");
  }
  if (Number(report.schema_version) >= 30 &&
    (report?.policy
      ?.both_lucky_families_require_observed_mitigation_inputs_before_route_selection !== true ||
      report?.attack_lucky_mitigation_diagnostic?.status !==
        "same-build-attack-lucky-mitigation-axes-unobserved" ||
      JSON.stringify(report?.attack_lucky_mitigation_diagnostic?.ability_ids) !==
        JSON.stringify([2031101, 2031103, 2031104, 2031107, 2031109, 2031110]) ||
      Number(report?.attack_lucky_mitigation_diagnostic?.hit_event_id) !== 3 ||
      Number(report?.attack_lucky_mitigation_diagnostic?.selected_sample_count) !== 84072 ||
      Number(report?.attack_lucky_mitigation_diagnostic?.physical_defense_axis_samples) !== 0 ||
      Number(report?.attack_lucky_mitigation_diagnostic?.magic_defense_axis_samples) !== 0 ||
      Number(report?.attack_lucky_mitigation_diagnostic?.controlled_mitigation_pairs) !== 0 ||
      report?.attack_lucky_mitigation_diagnostic?.remote_player_packet_dependency !== false ||
      report?.attack_lucky_mitigation_diagnostic?.absent_axes_are_zero_mitigation !== false ||
      report?.attack_lucky_mitigation_diagnostic?.exact_target_mitigation_formula_proven !== false ||
      report?.attack_lucky_mitigation_diagnostic
        ?.exact_operation_order_and_integer_rounding_proven !== false ||
      report?.attack_lucky_mitigation_diagnostic?.complete_status_baseline_proven !== false ||
      report?.attack_lucky_mitigation_diagnostic?.packet_conservation_proven !== false ||
      report?.attack_lucky_mitigation_diagnostic?.formula_authority !== false ||
      report?.attack_lucky_mitigation_diagnostic?.runtime_authority !== false ||
      report?.attack_lucky_mitigation_diagnostic?.ui_display_authority !== false ||
      report?.attack_lucky_mitigation_diagnostic?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.attack_lucky_selected_damage_results) !== 84072 ||
      Number(report?.summary?.attack_lucky_physical_defense_axis_samples) !== 0 ||
      Number(report?.summary?.attack_lucky_magic_defense_axis_samples) !== 0 ||
      Number(report?.summary?.both_lucky_families_selected_damage_results) !== 125183)) {
    throw new Error("Blade Sweep scalar proof has unsafe AttackLucky mitigation evidence");
  }
  if (Number(report.schema_version) === 31 &&
    (report?.policy
      ?.complete_observed_lucky_parent_binding_does_not_invent_multiplier_formula_semantics !== true ||
      report?.lucky_parent_multiplier_proof?.status !==
        "exact-current-build-lucky-parent-complete-obvious-multiplier-candidates-rejected" ||
      Number(report?.lucky_parent_multiplier_proof?.source_rlog_count) !== 26 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_ability_id) !== 2031109 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_hit_event_id) !== 3 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_events) !== 1692 ||
      String(report?.lucky_parent_multiplier_proof?.lucky_observed_damage) !== "59479454" ||
      Number(report?.lucky_parent_multiplier_proof?.immediate_same_wire_parent_events) !== 1692 ||
      Number(report?.lucky_parent_multiplier_proof?.unresolved_parent_events) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.ambiguous_parent_events) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.multiplier_candidate_events) !== 1289 ||
      Number(report?.lucky_parent_multiplier_proof?.multiplier_candidate_exact_matches) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.tested_multiplier_formulas_rejected) !== 2 ||
      report?.lucky_parent_multiplier_proof?.remote_player_packet_dependency !== false ||
      report?.lucky_parent_multiplier_proof?.parent_relation_for_observed_subset_proven !== true ||
      report?.lucky_parent_multiplier_proof?.lucky_multiplier_formula_proven !== false ||
      report?.lucky_parent_multiplier_proof?.general_lucky_formula_semantics_proven !== false ||
      report?.lucky_parent_multiplier_proof?.formula_authority !== false ||
      report?.lucky_parent_multiplier_proof?.runtime_authority !== false ||
      report?.lucky_parent_multiplier_proof?.ui_display_authority !== false ||
      report?.lucky_parent_multiplier_proof?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.lucky_parent_observed_events) !== 1692 ||
      Number(report?.summary?.lucky_parent_unresolved_events) !== 0 ||
      Number(report?.summary?.lucky_multiplier_candidate_events) !== 1289 ||
      Number(report?.summary?.lucky_multiplier_candidate_exact_matches) !== 0)) {
    throw new Error("Blade Sweep scalar proof has unsafe Lucky parent or multiplier evidence");
  }
  if (Number(report.schema_version) === 32 &&
    (report?.policy
      ?.complete_observed_lucky_parent_binding_does_not_invent_multiplier_formula_semantics !== true ||
      report?.lucky_parent_multiplier_proof?.status !==
        "exact-current-build-lucky-parent-complete-recorded-multiplier-candidates-rejected" ||
      Number(report?.lucky_parent_multiplier_proof?.source_rlog_count) !== 26 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_ability_id) !== 2031109 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_hit_event_id) !== 3 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_events) !== 1692 ||
      String(report?.lucky_parent_multiplier_proof?.lucky_observed_damage) !== "59479454" ||
      Number(report?.lucky_parent_multiplier_proof?.immediate_same_wire_parent_events) !== 1692 ||
      Number(report?.lucky_parent_multiplier_proof?.unresolved_parent_events) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.ambiguous_parent_events) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.multiplier_candidate_events) !== 1289 ||
      Number(report?.lucky_parent_multiplier_proof?.multiplier_candidate_exact_matches) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attack_candidate_events) !== 1289 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attack_candidate_exact_matches) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.source_magic_attack_candidate_events) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.relation_groups) !== 52 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attack_candidate_minimum_residual) !== 1823 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attack_candidate_maximum_residual) !== 84858 ||
      Number(report?.lucky_parent_multiplier_proof
        ?.tested_multiplier_formulas_with_observations_rejected) !== 3 ||
      report?.lucky_parent_multiplier_proof?.remote_player_packet_dependency !== false ||
      report?.lucky_parent_multiplier_proof?.parent_relation_for_observed_subset_proven !== true ||
      report?.lucky_parent_multiplier_proof?.lucky_multiplier_formula_proven !== false ||
      report?.lucky_parent_multiplier_proof?.general_lucky_formula_semantics_proven !== false ||
      report?.lucky_parent_multiplier_proof?.formula_authority !== false ||
      report?.lucky_parent_multiplier_proof?.runtime_authority !== false ||
      report?.lucky_parent_multiplier_proof?.ui_display_authority !== false ||
      report?.lucky_parent_multiplier_proof?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.lucky_parent_observed_events) !== 1692 ||
      Number(report?.summary?.lucky_parent_unresolved_events) !== 0 ||
      Number(report?.summary?.lucky_multiplier_candidate_events) !== 1289 ||
      Number(report?.summary?.lucky_multiplier_candidate_exact_matches) !== 0 ||
      Number(report?.summary?.lucky_source_attack_candidate_exact_matches) !== 0 ||
      Number(report?.summary?.lucky_source_attack_relation_groups) !== 52)) {
    throw new Error("Blade Sweep scalar proof has unsafe grouped Lucky relation evidence");
  }
  if (Number(report.schema_version) >= 33 &&
    (report?.policy
      ?.complete_observed_lucky_parent_binding_does_not_invent_multiplier_formula_semantics !== true ||
      report?.policy
        ?.complete_local_source_attribute_candidate_exhaustion_does_not_invent_multi_input_formula_semantics !== true ||
      report?.lucky_parent_multiplier_proof?.status !==
        "exact-current-build-lucky-parent-complete-all-local-single-attribute-multiplier-candidates-rejected" ||
      Number(report?.lucky_parent_multiplier_proof?.source_rlog_count) !== 26 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_ability_id) !== 2031109 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_hit_event_id) !== 3 ||
      Number(report?.lucky_parent_multiplier_proof?.lucky_events) !== 1692 ||
      String(report?.lucky_parent_multiplier_proof?.lucky_observed_damage) !== "59479454" ||
      Number(report?.lucky_parent_multiplier_proof?.immediate_same_wire_parent_events) !== 1692 ||
      Number(report?.lucky_parent_multiplier_proof?.unresolved_parent_events) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.ambiguous_parent_events) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.multiplier_candidate_events) !== 1289 ||
      Number(report?.lucky_parent_multiplier_proof?.multiplier_candidate_exact_matches) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attack_candidate_events) !== 1289 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attack_candidate_exact_matches) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.source_magic_attack_candidate_events) !== 0 ||
      Number(report?.lucky_parent_multiplier_proof?.relation_groups) !== 52 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attack_candidate_minimum_residual) !== 1823 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attack_candidate_maximum_residual) !== 84858 ||
      Number(report?.lucky_parent_multiplier_proof
        ?.tested_multiplier_formulas_with_observations_rejected) !== 3 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attribute_candidate_events) !== 1289 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attribute_candidate_pairs) !== 177361 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attribute_candidate_ids) !== 224 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attribute_full_coverage_ids) !== 67 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attribute_varying_ids) !== 164 ||
      Number(report?.lucky_parent_multiplier_proof
        ?.source_attribute_within_relation_group_varying_ids) !== 163 ||
      Number(report?.lucky_parent_multiplier_proof?.source_attribute_candidate_exact_matches) !== 0 ||
      report?.lucky_parent_multiplier_proof?.simple_local_single_attribute_candidate_family_exhausted !== true ||
      report?.lucky_parent_multiplier_proof?.remote_player_packet_dependency !== false ||
      report?.lucky_parent_multiplier_proof?.parent_relation_for_observed_subset_proven !== true ||
      report?.lucky_parent_multiplier_proof?.lucky_multiplier_formula_proven !== false ||
      report?.lucky_parent_multiplier_proof?.general_lucky_formula_semantics_proven !== false ||
      report?.lucky_parent_multiplier_proof?.formula_authority !== false ||
      report?.lucky_parent_multiplier_proof?.runtime_authority !== false ||
      report?.lucky_parent_multiplier_proof?.ui_display_authority !== false ||
      report?.lucky_parent_multiplier_proof?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.lucky_parent_observed_events) !== 1692 ||
      Number(report?.summary?.lucky_parent_unresolved_events) !== 0 ||
      Number(report?.summary?.lucky_multiplier_candidate_events) !== 1289 ||
      Number(report?.summary?.lucky_multiplier_candidate_exact_matches) !== 0 ||
      Number(report?.summary?.lucky_source_attack_candidate_exact_matches) !== 0 ||
      Number(report?.summary?.lucky_source_attack_relation_groups) !== 52 ||
      Number(report?.summary?.lucky_source_attribute_candidate_events) !== 1289 ||
      Number(report?.summary?.lucky_source_attribute_candidate_pairs) !== 177361 ||
      Number(report?.summary?.lucky_source_attribute_candidate_ids) !== 224 ||
      Number(report?.summary?.lucky_source_attribute_candidate_exact_matches) !== 0)) {
    throw new Error("Blade Sweep scalar proof has unsafe local source-attribute Lucky evidence");
  }
  if (Number(report.schema_version) >= 34 &&
    (report?.policy?.exact_character_sheet_transform_does_not_prove_combat_stage_binding !== true ||
      report?.target_defense_transform_boundary?.status !==
        "exact-current-season-character-sheet-defense-transform-combat-stage-unbound" ||
      Number(report?.target_defense_transform_boundary?.current_season_id) !== 3 ||
      report?.target_defense_transform_boundary?.table !== "FightAttrTranTable" ||
      report?.target_defense_transform_boundary?.field !== "DefPara" ||
      JSON.stringify(report?.target_defense_transform_boundary?.parameters) !==
        JSON.stringify([22000, 1, 1, 0, 0, 0, 0]) ||
      report?.target_defense_transform_boundary?.exact_current_season_expression !==
        "100 * raw / (raw + 22000)" ||
      Number(report?.target_defense_transform_boundary?.exact_current_season_curve_constant) !== 22000 ||
      report?.target_defense_transform_boundary?.character_sheet_row_selection_proven !== true ||
      report?.target_defense_transform_boundary?.character_sheet_operation_order_proven !== true ||
      report?.target_defense_transform_boundary?.character_sheet_underlying_rounding !== "none" ||
      report?.target_defense_transform_boundary?.combat_stage_binding_proven !== false ||
      report?.target_defense_transform_boundary?.effect_reduces_raw_defense_before_transform_proven !== false ||
      report?.target_defense_transform_boundary?.server_integer_rounding_proven !== false ||
      report?.target_defense_transform_boundary?.packet_conservation_proven !== false ||
      report?.target_defense_transform_boundary?.formula_authority !== false ||
      report?.target_defense_transform_boundary?.runtime_authority !== false ||
      report?.target_defense_transform_boundary?.ui_rdps_display_authority !== false ||
      report?.target_defense_transform_boundary?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.current_season_id) !== 3 ||
      Number(report?.summary?.exact_current_season_defense_curve_constant) !== 22000 ||
      report?.summary?.character_sheet_defense_transform_operation_order_proven !== true ||
      report?.summary?.combat_defense_transform_stage_binding_proven !== false)) {
    throw new Error("Blade Sweep scalar proof has unsafe target defense transform boundary evidence");
  }
  if (Number(report.schema_version) >= 11 &&
    (report?.policy?.produced_action_routes_do_not_prove_status_modifier_damage_neutrality !== true ||
      report?.target_status_action_route_audit?.status !==
        "exact-produced-action-routes-audited-status-modifier-neutrality-unproven" ||
      Number(report?.target_status_action_route_audit?.audited_effects) !== 12 ||
      Number(report?.target_status_action_route_audit?.produced_damage_action_effects) !== 0 ||
      Number(report?.target_status_action_route_audit?.produced_action_healing_only_effects) !== 3 ||
      Number(report?.target_status_action_route_audit?.no_produced_action_observed_effects) !== 9 ||
      Number(report?.target_status_action_route_audit?.effects_eliminated_as_damage_neutral) !== 0 ||
      JSON.stringify(report?.target_status_action_route_audit
        ?.candidate_near_pair_status_effects_without_same_axis_witness) !==
        JSON.stringify([55301, 2201452]) ||
      report?.target_status_action_route_audit?.status_modifier_damage_neutrality_proven !== false ||
      report?.target_status_action_route_audit?.target_status_confounders_eliminated !== false ||
      report?.target_status_action_route_audit?.formula_authority !== false ||
      report?.target_status_action_route_audit?.runtime_authority !== false ||
      report?.target_status_action_route_audit?.ui_display_authority !== false ||
      report?.target_status_action_route_audit?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.target_status_action_route_audited_effects) !== 12 ||
      Number(report?.summary?.target_status_confounders_eliminated_by_action_routes) !== 0)) {
    throw new Error("Blade Sweep scalar proof has unsafe target-status action-route evidence");
  }
  if (Number(report.schema_version) >= 12 &&
    (report?.policy?.exact_defense_stat_formula_does_not_prove_target_defense_to_damage_projection !== true ||
      report?.target_defense_percent_lifecycle_proof?.status !==
        "defense-stat-formula-proven-damage-counterfactual-unproven" ||
      Number(report?.target_defense_percent_lifecycle_proof?.effect_id) !== 2201452 ||
      Number(report?.target_defense_percent_lifecycle_proof?.attribute_id) !== 11350 ||
      Number(report?.target_defense_percent_lifecycle_proof?.percent_basis_points) !== 1000 ||
      Number(report?.target_defense_percent_lifecycle_proof?.exact_wire_occurrences) !== 51 ||
      Number(report?.target_defense_percent_lifecycle_proof?.application_occurrences) !== 30 ||
      Number(report?.target_defense_percent_lifecycle_proof?.removal_occurrences) !== 21 ||
      Number(report?.target_defense_percent_lifecycle_proof?.independent_sessions) !== 13 ||
      report?.target_defense_percent_lifecycle_proof
        ?.effect_2201452_exact_defense_axis_mechanism_proven !== true ||
      report?.target_defense_percent_lifecycle_proof
        ?.exact_target_defense_to_damage_formula_proven !== false ||
      report?.target_defense_percent_lifecycle_proof
        ?.effect_2201452_damage_stage_exclusivity_proven !== false ||
      report?.target_defense_percent_lifecycle_proof
        ?.hidden_additional_damage_stage_behavior_excluded !== false ||
      report?.target_defense_percent_lifecycle_proof?.formula_authority !== false ||
      report?.target_defense_percent_lifecycle_proof?.runtime_authority !== false ||
      report?.target_defense_percent_lifecycle_proof?.ui_display_authority !== false ||
      report?.target_defense_percent_lifecycle_proof?.provider_rdps_credit_allowed !== false ||
      report?.summary?.exact_effect_2201452_defense_stat_formula_proven !== true ||
      Number(report?.summary?.effect_2201452_exact_wire_transition_occurrences) !== 51 ||
      Number(report?.summary?.effect_2201452_exact_wire_independent_sessions) !== 13)) {
    throw new Error("Blade Sweep scalar proof has unsafe target defense lifecycle evidence");
  }
  if (Number(report.schema_version) >= 13 &&
    (report?.policy
      ?.defense_final_only_observations_are_preserved_without_claiming_raw_percent_packet_visibility !== true ||
      Number(report?.target_defense_percent_lifecycle_proof?.packet_raw_percent_joined_occurrences) !== 47 ||
      Number(report?.target_defense_percent_lifecycle_proof?.final_only_unresolved_occurrences) !== 4 ||
      Number(report?.target_defense_percent_lifecycle_proof?.exact_family_input_transitions) !== 158 ||
      Number(report?.target_defense_percent_lifecycle_proof?.nearest_rounding_residual_mismatches) !== 86 ||
      report?.target_defense_percent_lifecycle_proof?.truncation_selected_over_round_to_nearest !== true ||
      report?.target_defense_percent_lifecycle_proof
        ?.raw_percent_identity_for_all_lifecycle_occurrences_proven !== false ||
      Number(report?.summary?.effect_2201452_packet_raw_percent_joined_occurrences) !== 47 ||
      Number(report?.summary?.effect_2201452_final_only_unresolved_occurrences) !== 4)) {
    throw new Error("Blade Sweep scalar proof has unsafe raw-percent lifecycle evidence");
  }
  if (Number(report.schema_version) >= 14 &&
    (report?.policy?.complete_observed_fight_attribute_scope_does_not_exclude_hidden_damage_logic !== true ||
      report?.target_defense_fight_attribute_scope_proof?.status !==
        "complete-observed-fight-attribute-scope-hidden-damage-logic-unexcluded" ||
      Number(report?.target_defense_fight_attribute_scope_proof?.selected_fight_attribute_components) !== 906 ||
      Number(report?.target_defense_fight_attribute_scope_proof
        ?.components_with_exact_single_effect_same_wire_correlations) !== 26 ||
      Number(report?.target_defense_fight_attribute_scope_proof
        ?.proven_reversible_constant_components) !== 1 ||
      Number(report?.target_defense_fight_attribute_scope_proof
        ?.unresolved_fight_attribute_components) !== 25 ||
      Number(report?.target_defense_fight_attribute_scope_proof
        ?.only_proven_reversible_constant_attribute_id) !== 11354 ||
      Number(report?.target_defense_fight_attribute_scope_proof
        ?.raw_percent_basis_points_per_effect_presence) !== 1000 ||
      JSON.stringify(report?.target_defense_fight_attribute_scope_proof
        ?.unresolved_one_direction_attribute_ids) !== JSON.stringify([11710, 11711, 11712]) ||
      report?.target_defense_fight_attribute_scope_proof
        ?.effect_is_defense_stat_only_across_observed_fight_attribute_components_proven !== false ||
      report?.target_defense_fight_attribute_scope_proof?.hidden_damage_stage_behavior_excluded !== false ||
      report?.target_defense_fight_attribute_scope_proof?.formula_authority !== false ||
      report?.target_defense_fight_attribute_scope_proof?.runtime_authority !== false ||
      report?.target_defense_fight_attribute_scope_proof?.ui_display_authority !== false ||
      report?.target_defense_fight_attribute_scope_proof?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.effect_2201452_selected_fight_attribute_components) !== 906 ||
      Number(report?.summary?.effect_2201452_proven_reversible_constant_components) !== 1 ||
      Number(report?.summary?.effect_2201452_unresolved_fight_attribute_components) !== 25)) {
    throw new Error("Blade Sweep scalar proof has unsafe complete fight-attribute scope evidence");
  }
  if (Number(report.schema_version) >= 15 &&
    (report?.policy?.sparse_crit_co_updates_do_not_establish_an_unconditional_secondary_component !== true ||
      Number(report?.target_defense_fight_attribute_scope_proof?.raw_armor_presence_transitions) !== 47 ||
      Number(report?.target_defense_fight_attribute_scope_proof?.raw_armor_applications) !== 26 ||
      Number(report?.target_defense_fight_attribute_scope_proof?.raw_armor_removals) !== 21 ||
      Number(report?.target_defense_fight_attribute_scope_proof?.raw_crit_add_application_co_updates) !== 0 ||
      Number(report?.target_defense_fight_attribute_scope_proof?.raw_crit_add_removal_co_updates) !== 2 ||
      Number(report?.target_defense_fight_attribute_scope_proof
        ?.raw_armor_transitions_without_raw_crit_co_update) !== 45 ||
      Number(report?.target_defense_fight_attribute_scope_proof?.removal_only_raw_crit_add_delta) !== 50 ||
      report?.target_defense_fight_attribute_scope_proof
        ?.unconditional_fixed_negative_50_raw_crit_add_component_supported !== false ||
      report?.target_defense_fight_attribute_scope_proof
        ?.conditional_or_indirect_crit_behavior_excluded !== false ||
      Number(report?.summary?.effect_2201452_raw_armor_transitions_without_raw_crit_co_update) !== 45)) {
    throw new Error("Blade Sweep scalar proof has unsafe sparse Crit co-transition evidence");
  }
  if (Number(report.schema_version) >= 16 &&
    (report?.policy?.exhaustive_local_status_diagnostics_do_not_make_confounded_near_pairs_authoritative !== true ||
      report?.target_defense_status_diagnostic_rollup?.status !==
        "exhaustive-local-status-diagnostic-search-no-independent-control" ||
      Number(report?.target_defense_status_diagnostic_rollup?.matching_build_capture_diagnostics) !== 24 ||
      Number(report?.target_defense_status_diagnostic_rollup?.damage_samples) !== 735016 ||
      Number(report?.target_defense_status_diagnostic_rollup?.physical_defense_unique_near_pairs) !== 3 ||
      Number(report?.target_defense_status_diagnostic_rollup
        ?.physical_defense_pairs_with_selected_effect_in_status_delta) !== 3 ||
      Number(report?.target_defense_status_diagnostic_rollup
        ?.physical_defense_same_axis_pairs_with_selected_effect_in_status_delta) !== 0 ||
      report?.target_defense_status_diagnostic_rollup?.no_new_independent_local_control_was_found !== true ||
      report?.target_defense_status_diagnostic_rollup?.remote_player_packet_acquisition_required !== false ||
      report?.target_defense_status_diagnostic_rollup?.formula_authority !== false ||
      report?.target_defense_status_diagnostic_rollup?.runtime_authority !== false ||
      report?.target_defense_status_diagnostic_rollup?.ui_display_authority !== false ||
      report?.target_defense_status_diagnostic_rollup?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.effect_2201452_status_diagnostic_damage_samples) !== 735016 ||
      Number(report?.summary?.effect_2201452_physical_defense_near_pairs) !== 3 ||
      Number(report?.summary?.effect_2201452_near_pairs_with_effect_in_status_delta) !== 3 ||
      Number(report?.summary?.effect_2201452_same_axis_damage_witnesses) !== 0)) {
    throw new Error("Blade Sweep scalar proof has unsafe expanded target-defense status diagnostics");
  }
  if (Number(report.schema_version) >= 17 &&
    (report?.policy
      ?.actor_scene_cross_capture_exhaustion_does_not_make_actor_shape_formula_authoritative !== true ||
      report?.target_mitigation_actor_scene_exhaustion?.status !==
        "exact-local-actor-scene-exhausted-no-cross-capture-control" ||
      Number(report?.target_mitigation_actor_scene_exhaustion?.selected_ability_id) !== 823225 ||
      Number(report?.target_mitigation_actor_scene_exhaustion?.selected_ability_samples) !== 185 ||
      Number(report?.target_mitigation_actor_scene_exhaustion?.physical_defense_samples) !== 23 ||
      Number(report?.target_mitigation_actor_scene_exhaustion
        ?.physical_defense_samples_with_stable_target_actor_id) !== 0 ||
      Number(report?.target_mitigation_actor_scene_exhaustion?.cross_capture_actor_shape_pairs) !== 0 ||
      report?.target_mitigation_actor_scene_exhaustion
        ?.structurally_unavailable_remote_player_packets_are_not_required !== true ||
      report?.target_mitigation_actor_scene_exhaustion
        ?.missing_stable_remote_player_identity_is_preserved_not_synthesized !== true ||
      report?.target_mitigation_actor_scene_exhaustion?.formula_authority !== false ||
      report?.target_mitigation_actor_scene_exhaustion?.runtime_authority !== false ||
      report?.target_mitigation_actor_scene_exhaustion?.ui_display_authority !== false ||
      report?.target_mitigation_actor_scene_exhaustion?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.actor_scene_selected_ability_samples) !== 185 ||
      Number(report?.summary?.actor_scene_physical_defense_samples) !== 23 ||
      Number(report?.summary?.actor_scene_cross_capture_pairs) !== 0 ||
      Number(report?.summary?.actor_scene_stable_target_actor_ids) !== 0)) {
    throw new Error("Blade Sweep scalar proof has unsafe actor-scene exhaustion evidence");
  }
  if (Number(report.schema_version) >= 18 &&
    (report?.policy
      ?.complete_gap_bounded_lifecycle_windows_do_not_make_counterfactual_formula_authority !== true ||
      report?.rlog_gap_window_audit?.status !==
        "exact-gap-bounded-lifecycles-found-counterfactual-unproven" ||
      Number(report?.rlog_gap_window_audit?.source_rlog_count) !== 26 ||
      Number(report?.rlog_gap_window_audit?.data_gap_count) <= 0 ||
      Number(report?.rlog_gap_window_audit?.rlogs_with_data_gaps) !== 26 ||
      Number(report?.rlog_gap_window_audit?.complete_gap_bounded_lifecycle_count) !== 39 ||
      Number(report?.rlog_gap_window_audit?.complete_windows_with_damage_count) !== 39 ||
      Number(report?.rlog_gap_window_audit?.damage_events_while_active) !== 2277 ||
      Number(report?.rlog_gap_window_audit?.lifecycles_cut_by_data_quality_boundary) !== 51 ||
      !Array.isArray(report?.rlog_gap_window_audit?.complete_gap_bounded_windows) ||
      report.rlog_gap_window_audit.complete_gap_bounded_windows.length !== 39 ||
      report?.rlog_gap_window_audit?.exact_damage_projection_proven !== false ||
      report?.rlog_gap_window_audit?.exact_operation_order_proven !== false ||
      report?.rlog_gap_window_audit?.exact_integer_rounding_proven !== false ||
      report?.rlog_gap_window_audit?.packet_conservation_proven !== false ||
      report?.rlog_gap_window_audit?.formula_authority !== false ||
      report?.rlog_gap_window_audit?.runtime_authority !== false ||
      report?.rlog_gap_window_audit?.ui_display_authority !== false ||
      report?.rlog_gap_window_audit?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.effect_2110092_gap_bounded_complete_lifecycles) !== 39 ||
      Number(report?.summary?.effect_2110092_gap_bounded_windows_with_damage) !== 39 ||
      Number(report?.summary?.effect_2110092_gap_bounded_damage_events) !== 2277 ||
      Number(report?.summary?.effect_2110092_lifecycles_cut_by_data_quality_boundaries) !== 51)) {
    throw new Error("Blade Sweep scalar proof has unsafe gap-bounded RLOG lifecycle evidence");
  }
  if (Number(report.schema_version) >= 19 &&
    (report?.policy
      ?.transition_adjacent_candidate_search_never_grants_counterfactual_formula_authority !== true ||
      report?.rlog_transition_counterfactual_audit?.status !==
        "transition-adjacent-local-search-no-exact-observed-input-control" ||
      Number(report?.rlog_transition_counterfactual_audit?.opposite_state_recent_comparisons) !== 47626 ||
      Number(report?.rlog_transition_counterfactual_audit?.same_normalized_damage_context_pairs) !== 37 ||
      Number(report?.rlog_transition_counterfactual_audit?.exact_observed_input_candidate_pairs) !== 0 ||
      Number(report?.rlog_transition_counterfactual_audit?.strict_controlled_counterfactual_pairs) !== 0 ||
      report?.rlog_transition_counterfactual_audit?.remote_player_packet_dependency !== false ||
      report?.rlog_transition_counterfactual_audit?.formula_authority !== false ||
      report?.rlog_transition_counterfactual_audit?.runtime_authority !== false ||
      report?.rlog_transition_counterfactual_audit?.ui_display_authority !== false ||
      report?.rlog_transition_counterfactual_audit?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.effect_2110092_transition_opposite_state_comparisons) !== 47626 ||
      Number(report?.summary?.effect_2110092_transition_same_context_pairs) !== 37 ||
      Number(report?.summary?.effect_2110092_transition_exact_input_pairs) !== 0)) {
    throw new Error("Blade Sweep scalar proof has unsafe transition-adjacent counterfactual evidence");
  }
  if (Number(report.schema_version) >= 20 &&
    (Number(report?.rlog_transition_counterfactual_audit
      ?.same_context_pairs_with_only_target_current_hp_difference) !== 0 ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.same_context_source_attribute_difference_counts?.[474]) !== 37 ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.same_context_target_attribute_difference_counts?.[443]) !== 37 ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.same_context_target_attribute_difference_counts?.[474]) !== 37 ||
      Number(report?.rlog_transition_counterfactual_audit
        ?.same_context_target_attribute_difference_counts?.[11310]) !== 37 ||
      Number(report?.summary
        ?.effect_2110092_transition_only_current_hp_difference_pairs) !== 0)) {
    throw new Error("Blade Sweep scalar proof lost the transition mismatch frontier");
  }
  if (Number(report.schema_version) >= 21 &&
    (report?.policy?.opaque_attribute_wire_shape_and_timing_never_grant_semantic_exclusion !== true ||
      report?.rlog_opaque_attribute_audit?.status !==
        "opaque-attributes-443-474-structurally-characterized-semantic-exclusion-unproven" ||
      Number(report?.rlog_opaque_attribute_audit?.source_rlog_count) !== 26 ||
      Number(report?.rlog_opaque_attribute_audit?.canonical_event_count) !== 6411565 ||
      Number(report?.rlog_opaque_attribute_audit?.reset_boundary_count) !== 16247 ||
      report?.rlog_opaque_attribute_audit?.remote_player_packet_dependency !== false ||
      Number(report?.rlog_opaque_attribute_audit?.attribute_443?.observation_count) !== 71036 ||
      Number(report?.rlog_opaque_attribute_audit?.attribute_474?.observation_count) !== 266216 ||
      Number(report?.rlog_opaque_attribute_audit?.attribute_474?.pair_entry_count) !== 1502529 ||
      Number(report?.rlog_opaque_attribute_audit?.attribute_474
        ?.pair_entries_matching_session_entities) !== 1501983 ||
      report?.rlog_opaque_attribute_audit?.attribute_443
        ?.safe_to_exclude_from_counterfactual_matching !== false ||
      report?.rlog_opaque_attribute_audit?.attribute_474
        ?.safe_to_exclude_from_counterfactual_matching !== false ||
      report?.rlog_opaque_attribute_audit?.safe_to_exclude_from_counterfactual_matching !== false ||
      report?.rlog_opaque_attribute_audit?.formula_authority !== false ||
      report?.rlog_opaque_attribute_audit?.runtime_authority !== false ||
      report?.rlog_opaque_attribute_audit?.ui_display_authority !== false ||
      report?.rlog_opaque_attribute_audit?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.opaque_attribute_443_observations) !== 71036 ||
      Number(report?.summary?.opaque_attribute_474_observations) !== 266216 ||
      Number(report?.summary?.opaque_attribute_474_pair_entries) !== 1502529 ||
      Number(report?.summary?.opaque_attribute_474_pair_entries_matching_session_entities) !==
        1501983)) {
    throw new Error("Blade Sweep scalar proof lost the opaque attribute blocker");
  }
  if (Number(report.schema_version) >= 22 &&
    (report?.policy?.healing_only_source_action_does_not_grant_status_confounder_exclusion !== true ||
      report?.source_status_confounder_route_audit?.status !==
        "healing-action-route-proven-status-damage-neutrality-unproven" ||
      Number(report?.source_status_confounder_route_audit?.effect_id) !== 55342 ||
      Number(report?.source_status_confounder_route_audit?.linked_action_id) !== 25534201 ||
      Number(report?.source_status_confounder_route_audit?.packet_damage_results) !== 0 ||
      Number(report?.source_status_confounder_route_audit?.packet_healing_results) !== 22320 ||
      Number(report?.source_status_confounder_route_audit
        ?.same_context_source_status_difference_count) !== 33 ||
      Number(report?.source_status_confounder_route_audit
        ?.strict_controlled_counterfactual_pairs) !== 0 ||
      report?.source_status_confounder_route_audit
        ?.remote_player_packet_acquisition_required !== false ||
      report?.source_status_confounder_route_audit
        ?.status_modifier_damage_neutrality_proven !== false ||
      report?.source_status_confounder_route_audit
        ?.may_exclude_from_counterfactual_matching !== false ||
      report?.source_status_confounder_route_audit?.formula_authority !== false ||
      report?.source_status_confounder_route_audit?.runtime_authority !== false ||
      report?.source_status_confounder_route_audit?.ui_display_authority !== false ||
      report?.source_status_confounder_route_audit?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.source_status_55342_packet_healing_results) !== 22320 ||
      Number(report?.summary?.source_status_55342_same_context_difference_count) !== 33)) {
    throw new Error("Blade Sweep scalar proof lost the source-status 55342 confounder blocker");
  }
  if (Number(report.schema_version) >= 23 &&
    (report?.policy
      ?.locally_observed_dynamic_recipient_deltas_do_not_invent_remote_provider_inputs !== true ||
      report?.source_status_local_observable_audit?.status !==
        "local-external-stat-transfer-subset-observed-general-formula-unproven" ||
      Number(report?.source_status_local_observable_audit?.effect_701010_windows) !== 62935 ||
      Number(report?.source_status_local_observable_audit
        ?.effect_701010_unresolved_cross_actor_windows) !== 62935 ||
      Number(report?.source_status_local_observable_audit
        ?.effect_2207252_external_player_windows) !== 12948 ||
      Number(report?.source_status_local_observable_audit
        ?.effect_2207252_exact_agility_delta_occurrences) !== 48 ||
      Number(report?.source_status_local_observable_audit
        ?.effect_2207252_exact_agility_delta_independent_runs) !== 16 ||
      Number(report?.source_status_local_observable_audit
        ?.remote_provider_attribute_context_examples) !== 0 ||
      report?.source_status_local_observable_audit
        ?.remote_player_attribute_acquisition_required !== false ||
      report?.source_status_local_observable_audit
        ?.current_snapshot_substitution_allowed !== false ||
      report?.source_status_local_observable_audit?.general_transfer_percent_proven !== false ||
      report?.source_status_local_observable_audit?.integer_rounding_proven !== false ||
      report?.source_status_local_observable_audit?.exact_damage_projection_proven !== false ||
      report?.source_status_local_observable_audit
        ?.both_effects_remain_counterfactual_confounders !== true ||
      report?.source_status_local_observable_audit?.formula_authority !== false ||
      report?.source_status_local_observable_audit?.runtime_authority !== false ||
      report?.source_status_local_observable_audit?.ui_display_authority !== false ||
      report?.source_status_local_observable_audit?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.source_status_2207252_external_player_windows) !== 12948 ||
      Number(report?.summary?.source_status_2207252_exact_agility_delta_occurrences) !== 48 ||
      Number(report?.summary
        ?.source_status_2207252_remote_provider_attribute_context_examples) !== 0)) {
    throw new Error("Blade Sweep scalar proof lost the local source-status transfer frontier");
  }
  if (Number(report.schema_version) >= 24 &&
    (report?.policy
      ?.gap_bounded_formula_rows_without_target_defense_do_not_prove_the_mitigation_curve !== true ||
      report?.target_effect_formula_proof?.status !==
        "gap-bounded-target-effect-formula-input-absent" ||
      Number(report?.target_effect_formula_proof?.exact_effect_id) !== 2110092 ||
      Number(report?.target_effect_formula_proof?.complete_gap_bounded_lifecycles) !== 39 ||
      Number(report?.target_effect_formula_proof?.gap_audited_damage_window_memberships) !== 2277 ||
      Number(report?.target_effect_formula_proof?.gap_matched_unique_damage_events) !== 2277 ||
      Number(report?.target_effect_formula_proof?.formula_samples) !== 2211 ||
      Number(report?.target_effect_formula_proof?.gap_rows_excluded_by_wire_start_status) !== 66 ||
      Number(report?.target_effect_formula_proof?.target_physical_defense_samples) !== 0 ||
      report?.target_effect_formula_proof?.exact_armor_to_damage_equation_proven !== false ||
      report?.target_effect_formula_proof?.exact_operation_order_proven !== false ||
      report?.target_effect_formula_proof?.exact_integer_rounding_proven !== false ||
      report?.target_effect_formula_proof?.packet_conservation_proven !== false ||
      report?.target_effect_formula_proof?.formula_authority !== false ||
      report?.target_effect_formula_proof?.runtime_authority !== false ||
      report?.target_effect_formula_proof?.ui_display_authority !== false ||
      report?.target_effect_formula_proof?.provider_rdps_credit_allowed !== false ||
      Number(report?.summary?.effect_2110092_gap_audited_damage_window_memberships) !== 2277 ||
      Number(report?.summary?.effect_2110092_gap_matched_unique_damage_events) !== 2277 ||
      Number(report?.summary?.effect_2110092_gap_bounded_wire_start_formula_samples) !== 2211 ||
      Number(report?.summary?.effect_2110092_gap_bounded_rows_excluded_by_wire_start_status) !== 66 ||
      Number(report?.summary?.effect_2110092_gap_bounded_target_physical_defense_samples) !== 0)) {
    throw new Error("Blade Sweep scalar proof lost the gap-bounded target-effect formula frontier");
  }
  if (report?.target_mitigation_evidence?.status !== "no-controlled-target-mitigation-pairs" ||
    Number(report?.target_mitigation_evidence?.damage_samples) <= 0 ||
    Number(report?.target_mitigation_evidence?.audited_axis_samples) <= 0 ||
    Number(report?.target_mitigation_evidence?.controlled_groups) !== 0 ||
    Number(report?.target_mitigation_evidence?.maximum_measured_peak_working_set_bytes) <= 0 ||
    report?.target_mitigation_evidence?.exact_target_mitigation_formula_proven !== false ||
    report?.target_mitigation_evidence?.operation_order_and_integer_rounding_proven !== false ||
    report?.target_mitigation_evidence?.packet_conservation_proven !== false ||
    report?.target_mitigation_evidence?.formula_authority !== false ||
    report?.target_mitigation_evidence?.runtime_authority !== false ||
    report?.target_mitigation_evidence?.provider_rdps_credit_allowed !== false) {
    throw new Error("Blade Sweep scalar proof has unsafe target mitigation evidence");
  }
  if (report?.global_target_mitigation_evidence?.status !== "no-controlled-target-mitigation-pairs" ||
    Number(report?.global_target_mitigation_evidence?.matching_build_source_rlogs) <= 0 ||
    Number(report?.global_target_mitigation_evidence?.damage_samples) <
      Number(report?.target_mitigation_evidence?.damage_samples) ||
    Number(report?.global_target_mitigation_evidence?.audited_axis_samples) <
      Number(report?.target_mitigation_evidence?.audited_axis_samples) ||
    Number(report?.global_target_mitigation_evidence?.controlled_groups) !== 0 ||
    report?.global_target_mitigation_evidence?.exact_target_mitigation_formula_proven !== false ||
    report?.global_target_mitigation_evidence?.operation_order_and_integer_rounding_proven !== false ||
    report?.global_target_mitigation_evidence?.packet_conservation_proven !== false ||
    report?.global_target_mitigation_evidence?.formula_authority !== false ||
    report?.global_target_mitigation_evidence?.runtime_authority !== false ||
    report?.global_target_mitigation_evidence?.provider_rdps_credit_allowed !== false) {
    throw new Error("Blade Sweep scalar proof has unsafe global target mitigation evidence");
  }
  for (const input of Object.values(report.inputs ?? {})) validateFileDescriptor(input);
  assertSameRlogSet(report.provider_ownership.input_rlogs, report.counterfactual_projection.input_rlogs);
}

function scalarLadder(rows, skillId, blockKey, armorKey) {
  const selected = rows.filter((row) => Number(row.SkillId) === skillId).sort((a, b) => Number(a.Level) - Number(b.Level));
  if (selected.length !== EXPECTED_LEVELS.length) throw new Error(`Skill ${skillId} must have exactly five scalar rows`);
  return selected.map((row) => {
    const params = new Map((row.FloatParameter ?? []).map(([key, value]) => [String(key), Number(value)]));
    const level = Number(row.Level);
    const block = params.get(blockKey);
    const armor = params.get(armorKey);
    if (!Number.isSafeInteger(level) || !Number.isSafeInteger(block) || !Number.isSafeInteger(armor)) {
      throw new Error(`Skill ${skillId} level ${row.Level} lacks exact integer ${blockKey}/${armorKey} values`);
    }
    return {
      level,
      block_damage_reduction_bonus_raw_basis_points: block,
      block_damage_reduction_bonus_percent: block / 100,
      armor_penetration_raw_basis_points: armor,
      armor_penetration_percent: armor / 100,
    };
  });
}

function assertLadder(ladder, block, armor, label) {
  assertExactIntegers(ladder.map((row) => row.level), EXPECTED_LEVELS, `${label} levels`);
  assertExactIntegers(ladder.map((row) => row.block_damage_reduction_bonus_raw_basis_points), block, `${label} block ladder`);
  assertExactIntegers(ladder.map((row) => row.armor_penetration_raw_basis_points), armor, `${label} armor ladder`);
}

function semanticLabels(row) {
  return (row.SkillAttrDes ?? []).map((entry) => String(entry?.[0] ?? "")).filter(Boolean);
}

function requireLabels(actual, requiredLabels, label) {
  for (const requiredLabel of requiredLabels) {
    if (!actual.includes(requiredLabel)) throw new Error(`${label} is missing semantic label ${requiredLabel}`);
  }
}

function tableRows(table, label) {
  if (!table || Array.isArray(table) || typeof table !== "object") throw new Error(`${label} must be an object table`);
  return Object.values(table);
}

function exactRow(rows, key, value, label) {
  const matches = rows.filter((row) => String(row?.[key]) === String(value));
  if (matches.length !== 1) throw new Error(`${label} ${key}=${value} matched ${matches.length} rows`);
  return matches[0];
}

function assertSameRlogSet(left, right) {
  const normalize = (values) => values.map((entry) => `${normalizedPath(entry.path)}|${entry.bytes}|${entry.sha256}`).sort();
  if (JSON.stringify(normalize(left)) !== JSON.stringify(normalize(right))) {
    throw new Error("Provider ownership and counterfactual evidence do not use the same exact RLOG identities");
  }
}

function validateRlogDescriptor(input) {
  validateFileDescriptor(input);
  return { path: String(input.path), bytes: Number(input.bytes), sha256: String(input.sha256) };
}

function validateFileDescriptor(input) {
  if (!input || !String(input.path ?? "") || !Number.isSafeInteger(Number(input.bytes)) || Number(input.bytes) <= 0 ||
    !/^(sha256:)?[0-9a-f]{64}$/.test(String(input.sha256 ?? ""))) {
    throw new Error("Input descriptor lacks an exact path, byte length, or SHA-256");
  }
  return input;
}

function uniqueSortedNumbers(rows, key) {
  return [...new Set(rows.map((row) => Number(row[key])))].sort((a, b) => a - b);
}

function uniqueSortedStrings(rows, key) {
  return [...new Set(rows.map((row) => String(row[key])))].sort();
}

function assertExactIntegers(actual, expected, label) {
  const normalized = (actual ?? []).map(Number);
  if (normalized.some((entry) => !Number.isSafeInteger(entry)) || JSON.stringify(normalized) !== JSON.stringify(expected)) {
    throw new Error(`${label} expected ${JSON.stringify(expected)}, got ${JSON.stringify(normalized)}`);
  }
}

function normalizedPath(value) {
  return path.resolve(String(value)).replaceAll("\\", "/").toLowerCase();
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function orderedContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(stableStringify(copy)).digest("hex");
}

function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) =>
    `${JSON.stringify(key)}:${stableStringify(value[key])}`
  ).join(",")}}`;
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

function resolved(parsed, key) {
  return path.resolve(required(parsed, key));
}

function camelToSnake(value) {
  return value.replace(/[A-Z]/g, (character) => `_${character.toLowerCase()}`);
}

function numericString(value, label) {
  if (!/^\d+$/.test(String(value))) throw new Error(`${label} must be a numeric string`);
  return String(value);
}

function positiveInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${label} must be a positive integer`);
  return parsed;
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

function selfTest() {
  const ladder = [1, 2, 3, 4, 5].map((level) => ({
    Id: level,
    SkillId: DIRECT_SKILL_ID,
    Level: level,
    FloatParameter: [["attrPer", String(level * 150)], ["attrAdd", String(level * 130)]],
  }));
  const parsedLadder = scalarLadder(ladder, DIRECT_SKILL_ID, "attrPer", "attrAdd");
  assertLadder(parsedLadder, EXPECTED_BLOCK_BASIS_POINTS, EXPECTED_ARMOR_BASIS_POINTS, "fixture");
  const fixture = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: "1",
    effect_id: 2110092,
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      exact_input_hashes_are_embedded: true,
      aggregate_offline_exhaustion_is_not_combat_formula_proof: true,
      target_status_relaxed_near_pairs_are_not_combat_formula_proof: true,
      status_confounded_integer_candidate_compatibility_is_not_combat_formula_proof: true,
      structurally_unobservable_remote_player_packets_are_not_formula_acquisition_requirements: true,
      candidate_counterfactual_discriminants_never_grant_formula_or_ui_authority: true,
      exact_character_sheet_transform_does_not_prove_combat_stage_binding: true,
      exact_packet_component_and_coefficient_identity_do_not_prove_defense_stage_order: true,
      same_input_status_invariance_does_not_remove_common_target_status_confounders: true,
      produced_action_routes_do_not_prove_status_modifier_damage_neutrality: true,
      exact_defense_stat_formula_does_not_prove_target_defense_to_damage_projection: true,
      defense_final_only_observations_are_preserved_without_claiming_raw_percent_packet_visibility: true,
      complete_observed_fight_attribute_scope_does_not_exclude_hidden_damage_logic: true,
      sparse_crit_co_updates_do_not_establish_an_unconditional_secondary_component: true,
      exhaustive_local_status_diagnostics_do_not_make_confounded_near_pairs_authoritative: true,
      actor_scene_cross_capture_exhaustion_does_not_make_actor_shape_formula_authoritative: true,
      complete_gap_bounded_lifecycle_windows_do_not_make_counterfactual_formula_authority: true,
      gap_bounded_formula_rows_without_target_defense_do_not_prove_the_mitigation_curve: true,
      transition_adjacent_candidate_search_never_grants_counterfactual_formula_authority: true,
      attribute_443_474_and_target_current_hp_exclusion_is_diagnostic_only: true,
      lucky_packet_component_identity_does_not_prove_defense_dependency_or_formula_semantics: true,
      absent_observed_mitigation_axes_are_not_zero_mitigation_or_formula_proof: true,
      both_lucky_families_require_observed_mitigation_inputs_before_route_selection: true,
      complete_observed_lucky_parent_binding_does_not_invent_multiplier_formula_semantics: true,
      complete_local_source_attribute_candidate_exhaustion_does_not_invent_multi_input_formula_semantics: true,
      opaque_attribute_wire_shape_and_timing_never_grant_semantic_exclusion: true,
      healing_only_source_action_does_not_grant_status_confounder_exclusion: true,
      locally_observed_dynamic_recipient_deltas_do_not_invent_remote_provider_inputs: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    build_identity: {
      exhaustive_source_manifest_complete: true,
      exact_static_table_hash_binding_proven: true,
      decoded_table_bindings: [{}, {}, {}, {}],
    },
    inputs: { fixture: { path: "fixture.json", bytes: 1, sha256: "a".repeat(64) } },
    static_scalar: { exact_static_scalar_proven: true, ladders_exactly_equal: true },
    provider_ownership: {
      input_rlogs: [{ path: "a.rlog", bytes: 1, sha256: `sha256:${"b".repeat(64)}` }],
      events_with_prior_status_instance_player_owner: 29,
      events_with_same_wire_packet_player_owner: 17,
      exact_provider_ownership_for_every_event_proven: true,
    },
    provider_ownership_gap_worklist: {
      status: "exact-provider-ownership-proven",
      unresolved_status_events: 0,
      prior_status_instance_player_owned_status_events: 29,
      same_wire_packet_player_owned_status_events: 17,
      gap_groups: 0,
      exact_provider_ownership_proven: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    counterfactual_projection: {
      input_rlogs: [{ path: "a.rlog", bytes: 1, sha256: `sha256:${"b".repeat(64)}` }],
      exact_armor_to_damage_equation_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
    },
    target_mitigation_evidence: {
      status: "no-controlled-target-mitigation-pairs",
      damage_samples: 1,
      audited_axis_samples: 1,
      controlled_groups: 0,
      maximum_measured_peak_working_set_bytes: 1,
      exact_target_mitigation_formula_proven: false,
      operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    global_target_mitigation_evidence: {
      status: "no-controlled-target-mitigation-pairs",
      matching_build_source_rlogs: 1,
      damage_samples: 1,
      audited_axis_samples: 1,
      controlled_groups: 0,
      exact_target_mitigation_formula_proven: false,
      operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_mitigation_offline_exhaustion: {
      status: "exact-current-build-aggregate-offline-client-and-packet-search-exhausted-final-validation-required",
      packet_capture_proofs: 1,
      packet_source_rlogs: 1,
      packet_damage_samples: 1,
      packet_audited_axis_samples: 1,
      packet_samples_with_physical_or_refined_defense: 1,
      packet_samples_with_magic_defense: 1,
      packet_samples_with_elemental_defense: 0,
      controlled_counterfactual_pairs: 0,
      promoted_combat_formulas: 0,
      final_validation: [{}, {}],
      exact_target_mitigation_formula_proven: false,
      operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_mitigation_acquisition_worklist: {
      status: "acquisition-required-strict-controls-status-damage-relevance-observed",
      matching_build_capture_diagnostics: 1,
      damage_samples: 1,
      audited_axis_samples: 1,
      strict_controlled_groups: 0,
      target_status_relaxed_distinct_axis_pairs: 0,
      pairs_with_effect_in_target_status_delta: 0,
      global_same_axis_target_status_pairs: 5,
      global_same_axis_equal_output_pairs: 4,
      global_same_axis_divergent_output_pairs: 1,
      structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
      acquisition_contract: { required_controls: ["exact"] },
      exact_target_mitigation_formula_proven: false,
      operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_mitigation_near_pair_candidate: {
      status: "exact-integer-candidate-compatible-status-confounded",
      model_id: "target-physical-armor-counterfactual",
      transformed_curve_constant: 22000,
      runtime_simple_curve_constant: 6500,
      packet_near_pair_rows: 3,
      transformed_curve_compatible_rows: 3,
      transformed_curve_unique_shared_base_values: ["107006"],
      runtime_simple_curve_compatible_rows: 0,
      selected_blade_sweep_effect_2110092_in_status_delta: false,
      exact_status_state_equal: false,
      effect_2201452_damage_stage_exclusivity_proven: false,
      confounder_counterfactual_exhaustion: {
        matching_build_capture_proofs: 24,
        matching_build_source_rlogs: 26,
        damage_samples: 735016,
        target_locus_observed_samples: 3009,
        exact_target_locus_controlled_groups: 0,
        every_common_confounder_observed_at_target_locus: true,
        every_common_confounder_exactly_controlled_at_target_locus: false,
        common_status_confounders_eliminated: false,
      },
      same_axis_status_invariance: {
        physical_defense_same_axis_status_pairs: 5,
        physical_defense_same_axis_equal_output_pairs: 4,
        physical_defense_same_axis_divergent_output_pairs: 1,
        target_status_can_change_damage_outside_raw_defense: true,
        candidate_status_effect_ids_without_same_axis_witness: [55301, 2201452],
      },
      exact_target_mitigation_formula_proven: false,
      operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_defense_transform_boundary: {
      status: "exact-current-season-character-sheet-defense-transform-combat-stage-unbound",
      current_season_id: 3,
      table: "FightAttrTranTable",
      field: "DefPara",
      parameters: [22000, 1, 1, 0, 0, 0, 0],
      exact_evaluator_formula:
        "100 * raw * p3 / (raw * p2 + p1 + min(season_level * p4, p5) + min(role_level * p6, p7))",
      exact_current_season_expression: "100 * raw / (raw + 22000)",
      exact_current_season_curve_constant: 22000,
      character_sheet_row_selection_proven: true,
      character_sheet_operation_order_proven: true,
      character_sheet_underlying_rounding: "none",
      character_sheet_display_truncation: "value - (value % 0.01)",
      combat_stage_binding_proven: false,
      effect_reduces_raw_defense_before_transform_proven: false,
      server_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_rdps_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    counterfactual_discriminants: {
      status: "exact-candidate-discriminants-awaiting-controlled-packet-proof",
      armor_penetration_basis_points: 650,
      defense_curve_constant: 22000,
      packet_formula_identity: {
        status: "exact-build-packet-occurrence-static-route-and-coefficient-bound",
        ability_id: 823225,
        hit_event_id: 3,
        packet_damage_source_id: 2,
        damage_attr_id: 282322503,
        pve_damage_ratio_basis_points: [25000],
        packet_damage_results: 185,
        coefficient_to_pre_mitigation_base_formula_proven: false,
      },
      observed_baseline_curve: {
        status: "three-distinct-defense-points-share-exact-integer-base-status-control-absent",
        exact_curve_compatible_rows: 22,
        preserved_status_confounded_rows: 1,
        unique_shared_nonnegative_base: 107006,
        same_input_status_invariance: {
          compatible_target_status_state_ids: 20,
          common_effect_ids_across_all_compatible_rows: Array.from({ length: 78 }, (_, i) => i),
          varying_effect_ids_across_all_compatible_rows: Array.from({ length: 36 }, (_, i) => 100 + i),
          same_input_groups: [
            {},
            { isolated_single_effect_toggle_receipts: [{ effect_id: 2203182 }] },
          ],
          isolated_single_effect_toggle_count: 1,
          common_target_status_confounders_remain: true,
          target_status_control_proven: false,
        },
        target_status_control_proven: false,
      },
      exact_discriminant_rows: [{}, {}],
      distinct_predicted_damage_with_effect: [85530, 85533, 87122, 87125],
      acquisition_contract: { remote_player_packet_dependency: false },
      exact_damage_projection_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_status_action_route_audit: {
      status: "exact-produced-action-routes-audited-status-modifier-neutrality-unproven",
      audited_effects: 12,
      produced_damage_action_effects: 0,
      produced_action_healing_only_effects: 3,
      no_produced_action_observed_effects: 9,
      effects_eliminated_as_damage_neutral: 0,
      candidate_near_pair_status_effects_without_same_axis_witness: [55301, 2201452],
      status_modifier_damage_neutrality_proven: false,
      target_status_confounders_eliminated: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_defense_percent_lifecycle_proof: {
      status: "defense-stat-formula-proven-damage-counterfactual-unproven",
      effect_id: 2201452,
      attribute_id: 11350,
      percent_basis_points: 1000,
      percent: 10,
      formula:
        "buffed_physical_defense = trunc(base_physical_defense * (10000 + percent_basis_points) / 10000)",
      exact_wire_occurrences: 51,
      packet_raw_percent_joined_occurrences: 47,
      final_only_unresolved_occurrences: 4,
      exact_family_input_transitions: 158,
      nearest_rounding_residual_mismatches: 86,
      truncation_selected_over_round_to_nearest: true,
      raw_percent_identity_for_all_lifecycle_occurrences_proven: false,
      application_occurrences: 30,
      removal_occurrences: 21,
      independent_sessions: 13,
      distinct_external_sources: 3,
      distinct_base_values: 5,
      effect_2201452_exact_defense_axis_mechanism_proven: true,
      exact_target_defense_to_damage_formula_proven: false,
      effect_2201452_damage_stage_exclusivity_proven: false,
      hidden_additional_damage_stage_behavior_excluded: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_defense_fight_attribute_scope_proof: {
      status: "complete-observed-fight-attribute-scope-hidden-damage-logic-unexcluded",
      effect_id: 2201452,
      selected_fight_attribute_components: 906,
      components_with_exact_single_effect_same_wire_correlations: 26,
      proven_reversible_constant_components: 1,
      unresolved_one_direction_constant_components: 3,
      unresolved_nonstationary_components: 22,
      unresolved_fight_attribute_components: 25,
      only_proven_reversible_constant_attribute_id: 11354,
      raw_percent_basis_points_per_effect_presence: 1000,
      raw_percent_exact_occurrences: 47,
      raw_percent_independent_sessions: 13,
      unresolved_one_direction_attribute_ids: [11710, 11711, 11712],
      raw_armor_presence_transitions: 47,
      raw_armor_applications: 26,
      raw_armor_removals: 21,
      raw_crit_add_application_co_updates: 0,
      raw_crit_add_removal_co_updates: 2,
      raw_armor_transitions_without_raw_crit_co_update: 45,
      removal_only_raw_crit_add_delta: 50,
      unconditional_fixed_negative_50_raw_crit_add_component_supported: false,
      conditional_or_indirect_crit_behavior_excluded: false,
      effect_is_defense_stat_only_across_observed_fight_attribute_components_proven: false,
      hidden_damage_stage_behavior_excluded: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_defense_status_diagnostic_rollup: {
      status: "exhaustive-local-status-diagnostic-search-no-independent-control",
      effect_id: 2201452,
      matching_build_capture_diagnostics: 24,
      unique_source_rlogs: 26,
      damage_samples: 735016,
      audited_axis_samples: 2294,
      physical_defense_axis_samples: 765,
      physical_defense_unique_near_pairs: 3,
      physical_defense_same_axis_status_pairs: 5,
      physical_defense_pairs_with_selected_effect_in_status_delta: 3,
      physical_defense_same_axis_pairs_with_selected_effect_in_status_delta: 0,
      diagnostics_with_physical_defense_near_pairs: 1,
      transformed_curve_constant: 22000,
      transformed_curve_compatible_rows: 3,
      runtime_simple_curve_constant: 6500,
      runtime_simple_curve_compatible_rows: 0,
      selected_effect_occurs_in_every_observed_physical_defense_near_pair: true,
      selected_effect_has_same_axis_damage_witness: false,
      no_new_independent_local_control_was_found: true,
      remote_player_packet_acquisition_required: false,
      exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_mitigation_actor_scene_exhaustion: {
      status: "exact-local-actor-scene-exhausted-no-cross-capture-control",
      selected_ability_id: 823225,
      selected_ability_samples: 185,
      physical_defense_samples: 23,
      physical_defense_capture_sessions: 1,
      physical_defense_samples_with_stable_target_actor_id: 0,
      cross_capture_actor_shape_pairs: 0,
      same_capture_status_confounded_near_pair_rows: 3,
      structurally_unavailable_remote_player_packets_are_not_required: true,
      missing_stable_remote_player_identity_is_preserved_not_synthesized: true,
      exact_target_mitigation_formula_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    rlog_gap_window_audit: {
      status: "exact-gap-bounded-lifecycles-found-counterfactual-unproven",
      source_rlog_count: 26,
      canonical_event_count: 1,
      data_gap_count: 1,
      rlogs_with_data_gaps: 26,
      complete_gap_bounded_lifecycle_count: 39,
      complete_windows_with_damage_count: 39,
      damage_events_while_active: 2277,
      lifecycles_cut_by_data_quality_boundary: 51,
      candidate_rlogs: ["fixture.rlog"],
      complete_gap_bounded_windows: Array.from({ length: 39 }, (_, index) => ({
        rlog: "fixture.rlog",
        applied_envelope_sequence: index * 2 + 1,
        terminal_envelope_sequence: index * 2 + 2,
        applied_observed_micros: index * 10,
        terminal_observed_micros: index * 10 + 1,
        damage_events_while_active: 1,
        gap_bounded: true,
        controlled_counterfactual_pair_proven: false,
        formula_authority: false,
      })),
      exact_damage_projection_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    target_effect_formula_proof: {
      status: "gap-bounded-target-effect-formula-input-absent",
      source_rlog_count: 26,
      exact_effect_id: 2110092,
      complete_gap_bounded_lifecycles: 39,
      gap_audited_damage_window_memberships: 2277,
      gap_matched_unique_damage_events: 2277,
      formula_samples: 2211,
      gap_rows_excluded_by_wire_start_status: 66,
      source_physical_attack_samples: 912,
      target_physical_defense_samples: 0,
      exact_armor_to_damage_equation_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    rlog_transition_counterfactual_audit: {
      status: "transition-adjacent-local-search-no-exact-observed-input-control",
      source_rlog_count: 26,
      canonical_event_count: 6411565,
      data_gap_count: 16181,
      damage_events: 735016,
      damage_events_with_selected_effect_active: 5463,
      opposite_state_recent_comparisons: 47626,
      same_normalized_damage_context_pairs: 37,
      same_context_and_observed_attribute_pairs: 0,
      same_context_and_nonselected_status_pairs: 0,
      same_context_pairs_with_only_target_current_hp_difference: 0,
      same_context_pairs_after_443_474_attribute_exclusion: 0,
      same_context_pairs_after_443_474_and_target_current_hp_exclusion: 1,
      same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses: 0,
      minimum_residual_observed_state_dimensions_after_443_474_exclusion: 6,
      minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion: 5,
      same_context_source_attribute_difference_counts: { 474: 37 },
      same_context_target_attribute_difference_counts: { 443: 37, 474: 37, 11310: 37 },
      same_context_source_temporary_attribute_difference_counts: {},
      same_context_target_temporary_attribute_difference_counts: {},
      same_context_source_status_difference_counts: {},
      same_context_target_status_difference_counts: {},
      exact_observed_input_candidate_pairs: 0,
      target_current_hp_excluded_candidate_pairs: 0,
      strict_controlled_counterfactual_pairs: 0,
      closest_residual_pair: {
        rlog: "runtime-data/logs/monitor-1787003553387.run-0005.rlog",
        session_id: "monitor-1787003553387.run-0005",
        segment_index: 266,
        present_sequence: 378384,
        absent_sequence: 378486,
        pair_gap_micros: 169986,
        source_actor_id: 4555,
        source_entity_uuid: 80976347776,
        target_actor_id: 4711,
        target_entity_uuid: 7075070016,
        ability_id: 2031105,
        present_amount: 308131,
        absent_amount: 308131,
        present_normal_value: null,
        absent_normal_value: null,
        source_attribute_ids: [474],
        target_attribute_ids: [443, 474, 11310],
        source_temporary_attribute_ids: [],
        target_temporary_attribute_ids: [],
        source_status_effect_ids: [55342, 2207252],
        target_status_effect_ids: [21432, 2203311, 2203521],
        only_target_current_hp_differs: false,
        residual_observed_state_dimensions_after_443_474_exclusion: 6,
        residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion: 5,
        source_target_attribute_snapshots_complete: false,
        selected_provider_exact: true,
        segment_status_baseline_complete: false,
        controlled_counterfactual_pair_proven: false,
        formula_authority: false,
      },
      remote_player_packet_dependency: false,
      exact_damage_projection_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    lucky_packet_component_proof: {
      status: "same-build-lucky-component-observed-nonstandard-formula-semantics-open",
      ledger_lucky_rows: 11,
      packet_observed_rows: 9,
      unobserved_ledger_rows: 2,
      packet_damage_results: 125183,
      explicit_lucky_value_exact_matches: 125183,
      packet_component_conservation_proven: true,
      selected_row: {
        damage_attr_id: 2203110503,
        lookup_key: "2031105:3",
        ability_id: 2031105,
        hit_event_id: 3,
        formula_family: "MAttackLucky",
        formula_signature_id: "formula-763154feff63c9b9",
        packet_damage_results: 7762,
        same_build_packet_occurrence_proven: true,
        packet_component_identity: "canonical-amount-equals-lucky-value",
        nonstandard_formula_semantics_proven: false,
        physical_defense_dependency_proven: false,
        magic_defense_dependency_proven: false,
        formula_authority: false,
        runtime_attribution_authority: false,
        ui_display_authority: false,
        provider_rdps_credit_allowed: false,
      },
      unobserved_damage_attr_ids: [2203110603, 2203110803],
      nonstandard_formula_semantics_proven: false,
      physical_or_magic_mitigation_route_proven: false,
      formula_authority: false,
      runtime_attribution_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    mattack_lucky_mitigation_diagnostic: {
      status: "same-build-mattack-lucky-mitigation-axes-unobserved",
      ability_ids: [2031102, 2031105, 2031111],
      hit_event_id: 3,
      selected_sample_count: 41111,
      samples_by_ability_id: { "2031102": 31284, "2031105": 7762, "2031111": 2065 },
      physical_defense_axis_samples: 0,
      magic_defense_axis_samples: 0,
      controlled_mitigation_pairs: 0,
      remote_player_packet_dependency: false,
      absent_axes_are_zero_mitigation: false,
      exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false,
      complete_status_baseline_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    attack_lucky_mitigation_diagnostic: {
      status: "same-build-attack-lucky-mitigation-axes-unobserved",
      ability_ids: [2031101, 2031103, 2031104, 2031107, 2031109, 2031110],
      hit_event_id: 3,
      selected_sample_count: 84072,
      samples_by_ability_id: {
        "2031101": 30281,
        "2031103": 35887,
        "2031104": 14684,
        "2031107": 874,
        "2031109": 1692,
        "2031110": 654,
      },
      physical_defense_axis_samples: 0,
      magic_defense_axis_samples: 0,
      controlled_mitigation_pairs: 0,
      remote_player_packet_dependency: false,
      absent_axes_are_zero_mitigation: false,
      exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false,
      complete_status_baseline_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    lucky_parent_multiplier_proof: {
      status: "exact-current-build-lucky-parent-complete-all-local-single-attribute-multiplier-candidates-rejected",
      source_rlog_count: 26,
      lucky_ability_id: 2031109,
      lucky_hit_event_id: 3,
      lucky_events: 1692,
      lucky_observed_damage: "59479454",
      immediate_same_wire_parent_events: 1692,
      unresolved_parent_events: 0,
      ambiguous_parent_events: 0,
      multiplier_candidate_events: 1289,
      multiplier_candidate_exact_matches: 0,
      source_attack_candidate_events: 1289,
      source_attack_candidate_exact_matches: 0,
      source_magic_attack_candidate_events: 0,
      relation_groups: 52,
      source_attack_candidate_minimum_residual: 1823,
      source_attack_candidate_maximum_residual: 84858,
      tested_multiplier_formulas_with_observations_rejected: 3,
      source_attribute_candidate_events: 1289,
      source_attribute_candidate_pairs: 177361,
      source_attribute_candidate_ids: 224,
      source_attribute_full_coverage_ids: 67,
      source_attribute_varying_ids: 164,
      source_attribute_within_relation_group_varying_ids: 163,
      source_attribute_candidate_exact_matches: 0,
      simple_local_single_attribute_candidate_family_exhausted: true,
      remote_player_packet_dependency: false,
      parent_relation_for_observed_subset_proven: true,
      lucky_multiplier_formula_proven: false,
      general_lucky_formula_semantics_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    rlog_opaque_attribute_audit: {
      status: "opaque-attributes-443-474-structurally-characterized-semantic-exclusion-unproven",
      source_rlog_count: 26,
      canonical_event_count: 6411565,
      reset_boundary_count: 16247,
      remote_player_packet_dependency: false,
      attribute_443: {
        observation_count: 71036,
        scalar_shape_observation_count: 71036,
        scalar_min: 0,
        scalar_max: 384000,
        most_common_signed_prior_delta: -22,
        most_common_signed_prior_delta_count: 3681,
        same_wire_related_damage_pairs: 399769,
        semantic_identity_proven: false,
        safe_to_exclude_from_counterfactual_matching: false,
      },
      attribute_474: {
        observation_count: 266216,
        pair_collection_shape_observation_count: 266216,
        pair_entry_count: 1502529,
        pair_entries_matching_session_entities: 1501983,
        distinct_pair_key_count: 764,
        distinct_pair_keys_matching_session_entities: 758,
        same_wire_related_damage_pairs: 1180203,
        semantic_identity_proven: false,
        safe_to_exclude_from_counterfactual_matching: false,
      },
      formula_input_semantics_proven: false,
      damage_consequence_semantics_proven: false,
      safe_to_exclude_from_counterfactual_matching: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    source_status_confounder_route_audit: {
      status: "healing-action-route-proven-status-damage-neutrality-unproven",
      effect_id: 55342,
      linked_action_id: 25534201,
      packet_damage_results: 0,
      packet_healing_results: 22320,
      same_context_source_status_difference_count: 33,
      strict_controlled_counterfactual_pairs: 0,
      remote_player_packet_acquisition_required: false,
      status_modifier_damage_neutrality_proven: false,
      may_exclude_from_counterfactual_matching: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    source_status_local_observable_audit: {
      status: "local-external-stat-transfer-subset-observed-general-formula-unproven",
      effect_701010_windows: 62935,
      effect_701010_unresolved_cross_actor_windows: 62935,
      effect_2207252_external_player_windows: 12948,
      effect_2207252_exact_agility_delta_occurrences: 48,
      effect_2207252_exact_agility_delta_independent_runs: 16,
      remote_provider_attribute_context_examples: 0,
      remote_player_attribute_acquisition_required: false,
      current_snapshot_substitution_allowed: false,
      general_transfer_percent_proven: false,
      integer_rounding_proven: false,
      exact_damage_projection_proven: false,
      both_effects_remain_counterfactual_confounders: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    summary: {
      observed_runtime_tier: 5,
      observed_runtime_armor_penetration_basis_points: 650,
      observed_runtime_armor_penetration_percent: 6.5,
      unresolved_provider_status_events: 0,
      same_axis_target_status_pairs: 5,
      same_axis_equal_output_pairs: 4,
      same_axis_divergent_output_pairs: 1,
      candidate_counterfactual_discriminant_rows: 2,
      candidate_counterfactual_distinct_output_signatures: 4,
      current_season_id: 3,
      exact_current_season_defense_curve_constant: 22000,
      character_sheet_defense_transform_operation_order_proven: true,
      combat_defense_transform_stage_binding_proven: false,
      exact_packet_damage_component_id: 282322503,
      exact_packet_component_coefficient_basis_points: 25000,
      actor_scene_curve_compatible_rows: 22,
      actor_scene_curve_distinct_defense_points: 3,
      actor_scene_curve_status_confounded_rows: 1,
      actor_scene_compatible_target_status_states: 20,
      actor_scene_common_target_status_confounders: 78,
      actor_scene_varying_target_status_effects: 36,
      actor_scene_isolated_invariant_single_effect_toggles: 1,
      target_status_action_route_audited_effects: 12,
      target_status_confounders_eliminated_by_action_routes: 0,
      exact_effect_2201452_defense_stat_formula_proven: true,
      effect_2201452_exact_wire_transition_occurrences: 51,
      effect_2201452_packet_raw_percent_joined_occurrences: 47,
      effect_2201452_final_only_unresolved_occurrences: 4,
      effect_2201452_selected_fight_attribute_components: 906,
      effect_2201452_proven_reversible_constant_components: 1,
      effect_2201452_unresolved_fight_attribute_components: 25,
      effect_2201452_raw_armor_transitions_without_raw_crit_co_update: 45,
      effect_2201452_exact_wire_independent_sessions: 13,
      effect_2201452_status_diagnostic_damage_samples: 735016,
      effect_2201452_physical_defense_near_pairs: 3,
      effect_2201452_near_pairs_with_effect_in_status_delta: 3,
      effect_2201452_same_axis_damage_witnesses: 0,
      actor_scene_selected_ability_samples: 185,
      actor_scene_physical_defense_samples: 23,
      actor_scene_cross_capture_pairs: 0,
      actor_scene_stable_target_actor_ids: 0,
      effect_2110092_gap_bounded_complete_lifecycles: 39,
      effect_2110092_gap_bounded_windows_with_damage: 39,
      effect_2110092_gap_bounded_damage_events: 2277,
      effect_2110092_lifecycles_cut_by_data_quality_boundaries: 51,
      effect_2110092_gap_audited_damage_window_memberships: 2277,
      effect_2110092_gap_matched_unique_damage_events: 2277,
      effect_2110092_gap_bounded_wire_start_formula_samples: 2211,
      effect_2110092_gap_bounded_rows_excluded_by_wire_start_status: 66,
      effect_2110092_gap_bounded_target_physical_defense_samples: 0,
      effect_2110092_transition_opposite_state_comparisons: 47626,
      effect_2110092_transition_same_context_pairs: 37,
      effect_2110092_transition_exact_input_pairs: 0,
      effect_2110092_transition_only_current_hp_difference_pairs: 0,
      effect_2110092_transition_pairs_after_443_474_exclusion: 0,
      effect_2110092_transition_pairs_after_443_474_and_target_current_hp_exclusion: 1,
      effect_2110092_transition_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses: 0,
      effect_2110092_transition_minimum_residual_dimensions_after_diagnostic_exclusions: 5,
      closest_transition_lucky_damage_attr_id: 2203110503,
      closest_transition_lucky_packet_damage_results: 7762,
      lucky_packet_component_damage_results: 125183,
      lucky_packet_component_exact_matches: 125183,
      mattack_lucky_selected_damage_results: 41111,
      mattack_lucky_physical_defense_axis_samples: 0,
      mattack_lucky_magic_defense_axis_samples: 0,
      attack_lucky_selected_damage_results: 84072,
      attack_lucky_physical_defense_axis_samples: 0,
      attack_lucky_magic_defense_axis_samples: 0,
      both_lucky_families_selected_damage_results: 125183,
      lucky_parent_observed_events: 1692,
      lucky_parent_unresolved_events: 0,
      lucky_multiplier_candidate_events: 1289,
      lucky_multiplier_candidate_exact_matches: 0,
      lucky_source_attack_candidate_exact_matches: 0,
      lucky_source_attack_relation_groups: 52,
      lucky_source_attribute_candidate_events: 1289,
      lucky_source_attribute_candidate_pairs: 177361,
      lucky_source_attribute_candidate_ids: 224,
      lucky_source_attribute_candidate_exact_matches: 0,
      opaque_attribute_443_observations: 71036,
      opaque_attribute_474_observations: 266216,
      opaque_attribute_474_pair_entries: 1502529,
      opaque_attribute_474_pair_entries_matching_session_entities: 1501983,
      source_status_55342_packet_healing_results: 22320,
      source_status_55342_same_context_difference_count: 33,
      source_status_2207252_external_player_windows: 12948,
      source_status_2207252_exact_agility_delta_occurrences: 48,
      source_status_2207252_remote_provider_attribute_context_examples: 0,
      exact_provider_ownership_proven: true,
      exact_damage_projection_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
  fixture.content_sha256 = contentHash(fixture);
  verifyReport(fixture);
  const unsafeCredit = structuredClone(fixture);
  unsafeCredit.summary.provider_rdps_credit_allowed = true;
  unsafeCredit.content_sha256 = contentHash(unsafeCredit);
  expectVerifyRejection(unsafeCredit, "provider credit");
  const unboundBuild = structuredClone(fixture);
  unboundBuild.build_identity.exact_static_table_hash_binding_proven = false;
  unboundBuild.content_sha256 = contentHash(unboundBuild);
  expectVerifyRejection(unboundBuild, "unbound static build identity");
  const unsafeOfflinePromotion = structuredClone(fixture);
  unsafeOfflinePromotion.target_mitigation_offline_exhaustion.promoted_combat_formulas = 1;
  unsafeOfflinePromotion.content_sha256 = contentHash(unsafeOfflinePromotion);
  expectVerifyRejection(unsafeOfflinePromotion, "offline exhaustion formula promotion");
  const unsafeNearPairAuthority = structuredClone(fixture);
  unsafeNearPairAuthority.target_mitigation_acquisition_worklist.formula_authority = true;
  unsafeNearPairAuthority.content_sha256 = contentHash(unsafeNearPairAuthority);
  expectVerifyRejection(unsafeNearPairAuthority, "target-status-relaxed near-pair authority");
  const unsafeCandidatePromotion = structuredClone(fixture);
  unsafeCandidatePromotion.target_mitigation_near_pair_candidate.formula_authority = true;
  unsafeCandidatePromotion.content_sha256 = contentHash(unsafeCandidatePromotion);
  expectVerifyRejection(unsafeCandidatePromotion, "status-confounded candidate promotion");
  const unsafeGapBoundedFormulaPromotion = structuredClone(fixture);
  unsafeGapBoundedFormulaPromotion.target_effect_formula_proof.formula_authority = true;
  unsafeGapBoundedFormulaPromotion.content_sha256 = contentHash(unsafeGapBoundedFormulaPromotion);
  expectVerifyRejection(unsafeGapBoundedFormulaPromotion, "gap-bounded formula promotion");
  const unsafeOpaqueAttributeExclusion = structuredClone(fixture);
  unsafeOpaqueAttributeExclusion.rlog_opaque_attribute_audit.safe_to_exclude_from_counterfactual_matching = true;
  unsafeOpaqueAttributeExclusion.content_sha256 = contentHash(unsafeOpaqueAttributeExclusion);
  expectVerifyRejection(unsafeOpaqueAttributeExclusion, "opaque attribute semantic exclusion");
  const unsafeSourceStatusExclusion = structuredClone(fixture);
  unsafeSourceStatusExclusion.source_status_confounder_route_audit
    .may_exclude_from_counterfactual_matching = true;
  unsafeSourceStatusExclusion.content_sha256 = contentHash(unsafeSourceStatusExclusion);
  expectVerifyRejection(unsafeSourceStatusExclusion, "source-status confounder exclusion");
  const unsafeRemoteProviderRequirement = structuredClone(fixture);
  unsafeRemoteProviderRequirement.source_status_local_observable_audit
    .remote_player_attribute_acquisition_required = true;
  unsafeRemoteProviderRequirement.content_sha256 = contentHash(unsafeRemoteProviderRequirement);
  expectVerifyRejection(unsafeRemoteProviderRequirement, "remote provider attribute requirement");
  console.log("bpsr-blade-sweep-scalar-proof self-test passed");
}

function expectVerifyRejection(report, label) {
  try {
    verifyReport(report);
  } catch {
    return;
  }
  throw new Error(`Self-test accepted unsafe ${label}`);
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-blade-sweep-scalar-proof.mjs analyze --build <id> --effect 2110092 --build-source-manifest <complete-build-source-manifest.json> --skill-table <SkillTable.json> --skill-effect-table <SkillEffectTable.json> --aoyi-star-table <SkillAoyiStarTable.json> --buff-table <BuffTable.json> --runtime-proof <json> --provider-ownership-proof <json> --provider-ownership-gap-worklist <json> --counterfactual-rollup <json> --target-mitigation-rollup <json> --global-target-mitigation-rollup <json> --target-mitigation-offline-exhaustion <json> --target-mitigation-acquisition-worklist <json> --target-mitigation-near-pair-candidate-proof <json> --counterfactual-discriminants <json> --fight-attribute-transform-surface <json> --fight-attribute-transform-evaluator-proof <json> --mastery-runtime-route-proof <json> --target-status-action-route-audit <json> --target-defense-percent-lifecycle-proof <json> --target-defense-fight-attribute-scope-proof <json> --target-defense-status-diagnostic-rollup <json> --target-mitigation-actor-scene-exhaustion <json> --rlog-gap-window-audit <json> --target-effect-formula-proof <json> --rlog-transition-counterfactual-audit <json> --rlog-opaque-attribute-audit <json> --source-status-confounder-route-audit <json> --source-status-local-observable-audit <json> --lucky-packet-component-proof <json> --mattack-lucky-mitigation-diagnostic <json> --attack-lucky-mitigation-diagnostic <json> --lucky-parent-multiplier-proof <json> --output <json>\n  node tools/bpsr-blade-sweep-scalar-proof.mjs verify --input <json>\n  node tools/bpsr-blade-sweep-scalar-proof.mjs self-test");
  process.exit(exitCode);
}
