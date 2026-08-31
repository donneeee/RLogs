#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const gameBuild = String(process.argv[2] ?? "24687926");
const inventoryBase = path.join(
  repoRoot,
  "plugins",
  "games",
  "blue-protocol-star-resonance",
  "research",
  "game-file-inventory",
  "global",
);
const buildRoot = path.join(inventoryBase, `steam-${gameBuild}`);
const priorRoot = path.join(inventoryBase, "steam-24609362");
const outputPath = process.argv[3]
  ? path.resolve(process.argv[3])
  : path.join(buildRoot, "imagine-formula-proof.v1.json");
const gameAssemblyPath = process.argv[4]
  ? path.resolve(process.argv[4])
  : "G:/SteamLibrary/steamapps/common/Blue Protocol Star Resonance/bpsr/GameAssembly.dll";
const dumpPath = process.argv[5]
  ? path.resolve(process.argv[5])
  : path.resolve(
      repoRoot,
      "..",
      ".codex_tmp",
      `il2cpp-current-${gameBuild}-full-output`,
      "dump.cs",
    );
const primaryAttackTransitionPath = process.argv[6]
  ? path.resolve(process.argv[6])
  : null;
const attackCounterfactualPaths = [process.argv[7], process.argv[8]]
  .filter(Boolean)
  .map((value) => path.resolve(value));
const attackDownstreamDiagnosticPath = process.argv[9]
  ? path.resolve(process.argv[9])
  : null;
const attackSourceStageOrderProofPath = process.argv[10]
  ? path.resolve(process.argv[10])
  : null;
const attackLifecycleAncestryProofPath = process.argv[11]
  ? path.resolve(process.argv[11])
  : null;
const attackLifecycleConditionedDiagnosticPath = process.argv[12]
  ? path.resolve(process.argv[12])
  : null;
const attackGroupRelativeDiagnosticPath = process.argv[13]
  ? path.resolve(process.argv[13])
  : null;
const attackCurrentHpDiagnosticPath = process.argv[14]
  ? path.resolve(process.argv[14])
  : null;
const attackIntervalClosedHpDiagnosticPath = process.argv[15]
  ? path.resolve(process.argv[15])
  : null;
const attackTargetStaticContextDiagnosticPath = process.argv[16]
  ? path.resolve(process.argv[16])
  : null;
const attackEffectiveStatWindowDiagnosticPath = process.argv[17]
  ? path.resolve(process.argv[17])
  : null;
const attackTargetMitigationActionIdentityPath = process.argv[18]
  ? path.resolve(process.argv[18])
  : null;
const attackTargetMitigationDiagnosticPath = process.argv[19]
  ? path.resolve(process.argv[19])
  : null;
const attackTargetMitigationOfflineExhaustionPath = process.argv[20]
  ? path.resolve(process.argv[20])
  : null;
const attackTargetMitigationControlledReplayPath = process.argv[21]
  ? path.resolve(process.argv[21])
  : null;
if (gameBuild === "24687926" &&
  (!primaryAttackTransitionPath || attackCounterfactualPaths.length !== 2 ||
    !attackDownstreamDiagnosticPath || !attackSourceStageOrderProofPath ||
    !attackLifecycleAncestryProofPath ||
    !attackLifecycleConditionedDiagnosticPath ||
    !attackGroupRelativeDiagnosticPath || !attackCurrentHpDiagnosticPath ||
    !attackIntervalClosedHpDiagnosticPath ||
    !attackTargetStaticContextDiagnosticPath ||
    !attackEffectiveStatWindowDiagnosticPath ||
    !attackTargetMitigationActionIdentityPath ||
    !attackTargetMitigationDiagnosticPath ||
    !attackTargetMitigationOfflineExhaustionPath ||
    !attackTargetMitigationControlledReplayPath)) {
  throw new Error(
    "Current build 24687926 requires the exact packet marginal proof as argument 6, two occurrence-scoped fail-closed event-time Attack counterfactual audits as arguments 7 and 8, the action-wide downstream diagnostic as argument 9, the exhaustive source-stage order proof as argument 10, the exact effect/action lifecycle ancestry receipt as argument 11, the lifecycle-conditioned damage observations as argument 12, the group-relative topology diagnostic as argument 13, the bounded wire-start CurrentHP diagnostic as argument 14, the interval-closed HP diagnostic as argument 15, the allegiance-neutral target static-context diagnostic as argument 16, the effective-stat-window-gated damage diagnostic as argument 17, the neutral mitigation action-identity proof as argument 18, its cross-capture mitigation diagnostic as argument 19, the exact-build offline consumer exhaustion proof as argument 20, and the controlled replay acquisition worklist as argument 21; refusing to regenerate from static or incomplete runtime candidates alone",
  );
}

const currentIdentityPath = path.join(buildRoot, "client-binary-identity.json");
const runtimeProofPath = path.join(buildRoot, "imagine-runtime-provider-recipient-proof.v1.json");
const primaryTransformPath = path.join(buildRoot, "primary-stat-attack-transform-proof.v1.json");
const primaryRuntimePath = path.join(buildRoot, "primary-attack-runtime-route-proof.v1.json");
const timeDecreePriorPath = path.join(priorRoot, "time-decree-component-proof.v1.json");
const superconductorPriorPath = path.join(
  priorRoot,
  "superconductor-surge-component-proof.v1.json",
);
const fatalSpiralTempAttributePath = path.join(
  buildRoot,
  "fatal-spiral-temp-attribute-audit.v1.json",
);
const fatalSpiralFamilyPath = path.join(
  buildRoot,
  "fatal-spiral-all-element-family-proof.v1.json",
);
const fatalSpiralCorrelationPath = path.join(
  buildRoot,
  "fatal-spiral-status-attribute-correlation.v1.json",
);

const currentIdentity = readJson(currentIdentityPath);
const runtimeProof = readJson(runtimeProofPath);
const primaryTransform = readJson(primaryTransformPath);
const primaryRuntime = readJson(primaryRuntimePath);
const timeDecreePrior = readJson(timeDecreePriorPath);
const superconductorPrior = readJson(superconductorPriorPath);
const fatalSpiralTempAttribute = readJson(fatalSpiralTempAttributePath);
const fatalSpiralFamily = readJson(fatalSpiralFamilyPath);
const fatalSpiralCorrelation = readJson(fatalSpiralCorrelationPath);
const primaryAttackTransition = primaryAttackTransitionPath
  ? readJson(primaryAttackTransitionPath)
  : null;
const attackCounterfactuals = attackCounterfactualPaths.map(readJson);
const attackDownstreamDiagnostic = attackDownstreamDiagnosticPath
  ? readJson(attackDownstreamDiagnosticPath)
  : null;
const attackSourceStageOrderProof = attackSourceStageOrderProofPath
  ? readJson(attackSourceStageOrderProofPath)
  : null;
const attackLifecycleAncestryProof = attackLifecycleAncestryProofPath
  ? readJson(attackLifecycleAncestryProofPath)
  : null;
const attackLifecycleConditionedDiagnostic = attackLifecycleConditionedDiagnosticPath
  ? readJson(attackLifecycleConditionedDiagnosticPath)
  : null;
const attackGroupRelativeDiagnostic = attackGroupRelativeDiagnosticPath
  ? readJson(attackGroupRelativeDiagnosticPath)
  : null;
const attackCurrentHpDiagnostic = attackCurrentHpDiagnosticPath
  ? readJson(attackCurrentHpDiagnosticPath)
  : null;
const attackIntervalClosedHpDiagnostic = attackIntervalClosedHpDiagnosticPath
  ? readJson(attackIntervalClosedHpDiagnosticPath)
  : null;
const attackTargetStaticContextDiagnostic = attackTargetStaticContextDiagnosticPath
  ? readJson(attackTargetStaticContextDiagnosticPath)
  : null;
const attackEffectiveStatWindowDiagnostic = attackEffectiveStatWindowDiagnosticPath
  ? readJson(attackEffectiveStatWindowDiagnosticPath)
  : null;
const attackTargetMitigationActionIdentity = attackTargetMitigationActionIdentityPath
  ? readJson(attackTargetMitigationActionIdentityPath)
  : null;
const attackTargetMitigationDiagnostic = attackTargetMitigationDiagnosticPath
  ? readJson(attackTargetMitigationDiagnosticPath)
  : null;
const attackTargetMitigationOfflineExhaustion =
  attackTargetMitigationOfflineExhaustionPath
    ? readJson(attackTargetMitigationOfflineExhaustionPath)
    : null;
const attackTargetMitigationControlledReplay =
  attackTargetMitigationControlledReplayPath
    ? readJson(attackTargetMitigationControlledReplayPath)
    : null;
const skillFightLevelTable = readJson(path.join(repoRoot, "Excels", "SkillFightLevelTable.json"));
const skillAoyiTable = readJson(path.join(repoRoot, "Excels", "SkillAoyiTable.json"));
const skillAoyiStarTable = readJson(path.join(repoRoot, "Excels", "SkillAoyiStarTable.json"));
const skillTable = readJson(path.join(repoRoot, "Excels", "SkillTable.json"));
const skillEffectTable = readJson(path.join(repoRoot, "Excels", "SkillEffectTable.json"));
const buffTable = readJson(path.join(repoRoot, "Excels", "BuffTable.json"));

assertBuild(currentIdentity, gameBuild, "client identity");
assertBuild(runtimeProof, gameBuild, "Imagine runtime proof");
assertBuild(primaryTransform, gameBuild, "primary-stat transform proof");
assertBuild(primaryRuntime, gameBuild, "primary-attack runtime route proof");
const primaryAttackPacketMarginal = verifyPrimaryAttackPacketMarginal(
  primaryAttackTransition,
  gameBuild,
);
const attackCounterfactualAudit = verifyAttackCounterfactualAudits(
  attackCounterfactuals,
  gameBuild,
);
const attackDownstreamAudit = verifyAttackDownstreamDiagnostic(
  attackDownstreamDiagnostic,
  gameBuild,
);
const attackSourceStageOrderAudit = verifyAttackSourceStageOrderProof(
  attackSourceStageOrderProof,
  attackDownstreamDiagnosticPath,
  gameBuild,
);
const attackLifecycleAudit = verifyAttackLifecycleAncestryProof(
  attackLifecycleAncestryProof,
  gameBuild,
);
const attackLifecycleConditionedAudit = verifyAttackLifecycleConditionedDiagnostic(
  attackLifecycleConditionedDiagnostic,
  attackDownstreamDiagnosticPath,
  attackLifecycleAncestryProofPath,
  gameBuild,
);
const attackGroupRelativeAudit = verifyAttackGroupRelativeDiagnostic(
  attackGroupRelativeDiagnostic,
  gameBuild,
);
const attackCurrentHpAudit = verifyAttackCurrentHpDiagnostic(
  attackCurrentHpDiagnostic,
  attackGroupRelativeDiagnosticPath,
  gameBuild,
);
const attackIntervalClosedHpAudit = verifyAttackIntervalClosedHpDiagnostic(
  attackIntervalClosedHpDiagnostic,
  attackGroupRelativeDiagnosticPath,
  gameBuild,
);
const attackTargetStaticContextAudit = verifyAttackTargetStaticContextDiagnostic(
  attackTargetStaticContextDiagnostic,
  attackGroupRelativeDiagnosticPath,
  gameBuild,
);
const attackEffectiveStatWindowAudit = verifyAttackEffectiveStatWindowDiagnostic(
  attackEffectiveStatWindowDiagnostic,
  primaryAttackTransitionPath,
  gameBuild,
);
const attackTargetMitigationAudit = verifyAttackTargetMitigationDiagnostic(
  attackTargetMitigationActionIdentity,
  attackTargetMitigationActionIdentityPath,
  attackTargetMitigationDiagnostic,
  gameBuild,
);
const attackTargetMitigationClosureAudit = verifyAttackTargetMitigationClosure(
  attackTargetMitigationOfflineExhaustion,
  attackTargetMitigationOfflineExhaustionPath,
  attackTargetMitigationControlledReplay,
  attackTargetMitigationControlledReplayPath,
  attackTargetMitigationAudit,
  gameBuild,
);

const tableProofs = {
  time_decree: await compareTables([
    ["skill_table", "SkillTable.json", timeDecreePrior.current_static_identity.skill_table_sha256],
    [
      "skill_effect_table",
      "SkillEffectTable.json",
      timeDecreePrior.current_static_identity.skill_effect_table_sha256,
    ],
    ["buff_table", "BuffTable.json", timeDecreePrior.current_static_identity.buff_table_sha256],
    [
      "skill_aoyi_star_table",
      "SkillAoyiStarTable.json",
      timeDecreePrior.current_static_identity.skill_aoyi_star_table_sha256,
    ],
    [
      "fight_attr_table",
      "FightAttrTable.json",
      timeDecreePrior.current_static_identity.fight_attr_table_sha256,
    ],
  ]),
  superconductor: await compareTables([
    [
      "skill_aoyi_table",
      "SkillAoyiTable.json",
      superconductorPrior.current_static_identity.skill_aoyi_table_sha256,
    ],
    [
      "buff_table",
      "BuffTable.json",
      superconductorPrior.current_static_identity.buff_table_sha256,
    ],
    [
      "attr_description",
      "AttrDescription.json",
      superconductorPrior.current_static_identity.attr_description_sha256,
    ],
  ]),
  fatal_spiral: await compareTables([
    [
      "skill_fight_level_table",
      "SkillFightLevelTable.json",
      "9f34c190d7c9ff03893e11835173cf9319e6e8028604b6cf72a5abdce4391967",
    ],
    [
      "skill_aoyi_star_table",
      "SkillAoyiStarTable.json",
      "b5680b2aa05204e2baa08b9eaa95244e6dcabc0e0d4f66062288911900206016",
    ],
    [
      "skill_table",
      "SkillTable.json",
      "2cb172bc819491b45e9bb160be7387dbb0f5107e04663cf67f19013da0403f75",
    ],
    [
      "skill_effect_table",
      "SkillEffectTable.json",
      "ebf7d200c1a70184d22c30a82990a377ed61b61254f51504df596dfb28eee88e",
    ],
    [
      "buff_table",
      "BuffTable.json",
      "d5f1380424947cdb8052d1bafb2f3a3541827819beb3030c7a3573ed44d6bb1c",
    ],
  ]),
};

const assemblyFingerprint = await fingerprint(gameAssemblyPath);
if (assemblyFingerprint.sha256 !== currentIdentity.game_assembly.sha256) {
  throw new Error(
    `Installed GameAssembly hash ${assemblyFingerprint.sha256} does not match current-build identity ${currentIdentity.game_assembly.sha256}`,
  );
}

const cooldownSignature = Buffer.from(
  "f30f104b38f30f58cef20f5c4340f20f5ac0f30f59c1f30f5e432cf30f584320eb030f28c6488bc3f30f1143",
  "hex",
);
const cooldownSignatureRva = 0x5392169;
const cooldownNativeProof = verifyNativeSignature({
  filePath: gameAssemblyPath,
  rva: cooldownSignatureRva,
  expected: cooldownSignature,
});
const dumpProof = verifyDumpContracts(dumpPath, [
  "public float InitProgress; // 0x20",
  "public float CdRealLen; // 0x2C",
  "public float CdAccelerateRate; // 0x38",
  "public double CdBeginTime; // 0x40",
  "// RVA: 0x5391FF0",
]);

const timeDecreeRuntime = runtimeComponent(
  runtimeProof,
  3921,
  "time-decree-external-cooldown-speed",
);
const superconductorStatsRuntime = runtimeComponent(
  runtimeProof,
  3971,
  "superconductor-surge-mechanical-power-main-stats",
);
const superconductorHealingRuntime = runtimeComponent(
  runtimeProof,
  3971,
  "superconductor-surge-mechanical-power-healing-received",
);
const fatalSpiralRuntime = runtimeComponent(
  runtimeProof,
  3957,
  "fatal-spiral-shared-all-element-bonus",
);
const fatalSpiralStatic = verifyFatalSpiralContracts({
  skillFightLevelTable,
  skillAoyiStarTable,
  skillTable,
  skillEffectTable,
  buffTable,
  familyProof: fatalSpiralFamily,
  correlationProof: fatalSpiralCorrelation,
  tempAttributeProof: fatalSpiralTempAttribute,
});
const superconductorStatic = verifySuperconductorContracts({
  skillFightLevelTable,
  skillAoyiTable,
  skillAoyiStarTable,
  skillTable,
  skillEffectTable,
  buffTable,
});
const currentClassPrimaryTransforms = (primaryTransform.active_class_routes ?? []).map((route) => {
  const family = (primaryTransform.families ?? []).find(
    (candidate) => candidate.transform_family_id === route.transform_family_id,
  );
  if (!family) {
    throw new Error(`Current-build transform family ${route.transform_family_id} is missing`);
  }
  if (Number(family.primary_attribute_id) !== Number(route.primary_attribute_id) ||
    Number(family.attack_attribute_id) !== Number(route.attack_attribute_id) ||
    Number(family.attack_add_attribute_id) !== Number(route.attack_add_attribute_id)) {
    throw new Error(`Current-build class ${route.class_id} transform route disagrees with its family`);
  }
  return {
    class_id: Number(route.class_id),
    class_name_evidence: String(route.class_name),
    primary_attribute_id: Number(route.primary_attribute_id),
    primary_attribute_name_evidence: String(route.primary_attribute_name),
    attack_attribute_id: Number(route.attack_attribute_id),
    attack_attribute_name_evidence: String(route.attack_attribute_name),
    attack_add_attribute_id: Number(route.attack_add_attribute_id),
    coefficient_basis_points: Number(family.coefficient_basis_points),
    fixed_point_denominator: Number(family.fixed_point_denominator),
    exact_ratio: String(family.exact_ratio),
    formula: String(family.formula),
    rounding: String(family.rounding),
    transform_family_id: String(route.transform_family_id),
    authority_scope:
      "static talent opcode and class route only; not by itself the complete packet marginal of a support effect",
    support_effect_packet_marginal_disposition: Number(route.class_id) === 11
      ? "effect 2110140 current-build packets require the separate packet_primary_attack_marginal_proof"
      : "not evaluated by the effect 2110140 packet proof",
  };
}).sort((left, right) => left.class_id - right.class_id);
if (currentClassPrimaryTransforms.length !== 9 ||
  new Set(currentClassPrimaryTransforms.map((route) => route.class_id)).size !== 9) {
  throw new Error("Current-build class-to-primary/attack transform coverage is incomplete");
}

const allTimeDecreeTablesExact = Object.values(tableProofs.time_decree).every(
  (proof) => proof.exact_match,
);
const allSuperconductorTablesExact = Object.values(tableProofs.superconductor).every(
  (proof) => proof.exact_match,
);
const allFatalSpiralTablesExact = Object.values(tableProofs.fatal_spiral).every(
  (proof) => proof.exact_match,
);
const cooldownEquationExact =
  cooldownNativeProof.exact_match && dumpProof.contracts.every((contract) => contract.present);
const primaryRouteExact =
  primaryTransform.proof_state === "offline-primary-stat-attack-transform-complete" &&
  Number(primaryTransform.summary?.remaining_supported_class_routes ?? -1) === 0 &&
  primaryRuntime.proof_state === "exact-current-build-canonical-runtime-input-route-proven" &&
  Number(primaryRuntime.summary?.canonical_code_contracts_satisfied ?? -1) ===
    Number(primaryRuntime.summary?.canonical_code_contracts ?? -2);

const components = [
  {
    imagine_skill_id: 3957,
    imagine_name: fatalSpiralStatic.imagine_name,
    component_id: "fatal-spiral-shared-all-element-bonus",
    effect_ids: [2110125],
    provider_marker_effect_ids: [2110124],
    excluded_owner_damage_ids: [111007400108],
    proof_state: allFatalSpiralTablesExact
      ? "current-build-tier-formula-and-packet-attribute-oracle-exact"
      : "current-build-formula-proof-incomplete",
    exact_component_scalar_available: allFatalSpiralTablesExact,
    matching_build_external_lifecycle_observed:
      Number(fatalSpiralRuntime.summary?.external_player_status_rows ?? 0) > 0,
    fixed_point_denominator: 10000,
    equation: "all_element_bonus_basis_points = 500 + tier_attr_per",
    base_attr_per: fatalSpiralStatic.base_attr_per,
    tier_values: fatalSpiralStatic.tier_values,
    duration_millis: fatalSpiralStatic.duration_millis,
    same_type_lockout_millis: 60000,
    packet_attribute_oracle: fatalSpiralStatic.packet_attribute_oracle,
    interpretation:
      "The shared party effect is transferable support value. Basilisk summon damage and caster-only transforms remain owner damage and are never transferred.",
    attribution_contract: {
      method: "effect-window-damage-counterfactual-only",
      effect_provider_and_recipient_lifecycle_complete: true,
      equipped_provider_tier_snapshot_required: true,
      affected_hit_rows_selected: false,
      integer_damage_counterfactual_complete: false,
      current_build_conservation_replay_complete: false,
      runtime_rdps_enabled: false,
    },
    current_runtime_summary: compactRuntimeSummary(fatalSpiralRuntime),
    remaining_proof_obligations: [
      "select recipient damage rows inside the exact effect 2110125 lifecycle",
      "apply the captured provider tier's 6/7/8/9/10 percent all-element scalar using the authoritative damage-stage rounding order",
      "exclude direct summon damage 111007400108 and all caster-only transforms from transferable credit",
      "prove recipient debit equals provider credit for the selected run segment",
    ],
  },
  {
    imagine_skill_id: 3921,
    imagine_name: timeDecreePrior.skill.name,
    component_id: "time-decree-external-cooldown-speed",
    effect_ids: [2110034],
    proof_state:
      allTimeDecreeTablesExact && cooldownEquationExact
        ? "current-build-static-scalar-and-native-equation-exact"
        : "current-build-formula-proof-incomplete",
    exact_component_scalar_available: allTimeDecreeTablesExact,
    exact_native_equation_available: cooldownEquationExact,
    matching_build_external_lifecycle_observed:
      Number(timeDecreeRuntime.summary?.external_player_status_rows ?? 0) > 0,
    tier_values: timeDecreePrior.current_static_identity.tier_cooldown_speed_percent,
    duration_millis: 20000,
    lockout_effect_id: 2110056,
    lockout_duration_millis: 60000,
    equation:
      "Progress = InitProgress + (Now - CdBeginTime) * (1 + CdAccelerateRate) / CdRealLen",
    interpretation:
      "Cooldown acceleration changes action opportunity. It is not a direct damage multiplier and never transfers overlapping damage.",
    attribution_contract: {
      method: "action-opportunity-counterfactual-only",
      qualifying_skill_cooldown_category_map_complete: false,
      recipient_cast_schedule_replay_complete: false,
      cooldown_enabled_extra_casts_identified: false,
      recounted_child_events_conserved: false,
      current_build_conservation_replay_complete: false,
      runtime_rdps_enabled: false,
    },
    current_runtime_summary: compactRuntimeSummary(timeDecreeRuntime),
    remaining_proof_obligations: [
      "map the exact cooldown category affected for every recipient action",
      "replay the recipient cast schedule with and without effect 2110034",
      "transfer only recounted child damage or healing from casts enabled by the shorter cooldown",
      "prove recipient debit equals provider credit for the selected run segment",
    ],
  },
  {
    imagine_skill_id: 3971,
    imagine_name: superconductorStatic.imagine_name_evidence,
    component_id: "superconductor-surge-mechanical-power-main-stats",
    effect_ids: [2110140],
    proof_state:
      allSuperconductorTablesExact && primaryRouteExact && primaryAttackPacketMarginal
        ? "current-build-tier-scalar-static-talent-routes-and-runtime-input-route-exact-packet-marginal-partial"
        : "current-build-formula-proof-incomplete",
    exact_component_scalar_available: allSuperconductorTablesExact && primaryRouteExact,
    matching_build_external_lifecycle_observed:
      Number(superconductorStatsRuntime.summary?.external_player_status_rows ?? 0) > 0,
    base_parameter_pair: superconductorStatic.base_parameter_pair,
    tier_parameter_pairs: superconductorStatic.tier_parameter_pairs,
    loadout_tier_parameter_pairs: superconductorStatic.loadout_tier_parameter_pairs,
    star_increment_parameter_pairs: superconductorStatic.star_increment_parameter_pairs,
    duration_millis: 15000,
    current_class_primary_transforms: currentClassPrimaryTransforms,
    packet_primary_attack_marginal_proof: primaryAttackPacketMarginal,
    packet_damage_counterfactual_audit: attackCounterfactualAudit,
    packet_action_wide_downstream_diagnostic: attackDownstreamAudit,
    packet_source_stage_order_diagnostic: attackSourceStageOrderAudit,
    packet_action_lifecycle_ancestry_receipt: attackLifecycleAudit,
    packet_lifecycle_conditioned_damage_diagnostic:
      attackLifecycleConditionedAudit,
    packet_group_relative_topology_diagnostic: attackGroupRelativeAudit,
    packet_current_hp_diagnostic: attackCurrentHpAudit,
    packet_interval_closed_hp_diagnostic: attackIntervalClosedHpAudit,
    packet_target_static_context_diagnostic: attackTargetStaticContextAudit,
    packet_effective_stat_window_damage_diagnostic: attackEffectiveStatWindowAudit,
    packet_target_mitigation_action_identity_and_curve_diagnostic:
      attackTargetMitigationAudit,
    packet_target_mitigation_offline_exhaustion_and_controlled_replay:
      attackTargetMitigationClosureAudit,
    historical_transition_guard: {
      historical_marksman_agility_to_attack_ratio: "58/100",
      current_build_effect_2110140_class_11_packet_ratio: "58/100",
      disposition:
        "current-build packets independently reproduce 58/100 at fifteen boundaries; the static 1/8 talent opcode is not the complete effect 2110140 packet marginal",
    },
    attribution_contract: {
      method: "exact-wire-attribute-delta-counterfactual-only",
      event_time_recipient_class_selects_transform: true,
      recipient_pre_effect_attribute_snapshot_complete: false,
      recipient_effect_attribute_delta_replay_complete: false,
      exact_packet_transition_boundaries_complete_for_retained_lifecycles: true,
      retained_lifecycle_count_with_exact_transition_boundaries: 8,
      retained_effective_stat_window_damage_actions: 12547,
      status_presence_only_damage_actions_reclassified_ambiguous: 1841,
      effective_stat_gated_stable_source_control_overlaps: 0,
      target_mitigation_actions_with_event_time_target_actor: 774,
      target_mitigation_actions_with_event_time_source_actor: 773,
      target_mitigation_actions_with_event_time_scene: 764,
      target_mitigation_physical_defense_contexts: 747,
      target_mitigation_magic_defense_contexts: 747,
      target_mitigation_refined_defense_contexts: 756,
      target_mitigation_controlled_axis_pairs: 0,
      target_mitigation_client_combat_consumers_proven: 0,
      target_mitigation_controlled_replay_actions: 774,
      target_mitigation_formulas_promoted: 0,
      packet_primary_attack_marginal_complete_for_all_boundaries: false,
      event_time_attack_component_snapshot_complete: false,
      retained_tier0_transition_window_damage_actions: 133,
      occurrence_scoped_final_attack_delta_complete_for_retained_tier0_actions: true,
      retained_tier0_actions_with_unique_damage_row_and_stage: 133,
      retained_tier0_actions_with_exact_conserved_attack_stage_share: 133,
      retained_tier0_actions_with_one_exact_integer_counterfactual: 0,
      action_wide_damage_2220352105_samples: 19858,
      action_wide_single_integer_factor_model_rejections: 5683,
      four_source_stage_observations: 14810,
      exhaustive_four_source_stage_candidate_models: 3840,
      exhaustive_four_source_stage_zero_rejection_models: 0,
      best_four_source_stage_model_rejections: 11105,
      optional_source_attribute_presence_partitions: 5,
      optional_source_attribute_presence_partitions_with_zero_rejection_model: 0,
      exact_action_rows_with_matching_preceding_effect_endpoint: 2709,
      exact_source_stage_observations_with_lifecycle_context: 2406,
      lifecycle_conditioned_conflicting_repeated_contexts: 152,
      lifecycle_and_target_conditioned_conflicting_repeated_contexts: 141,
      lifecycle_target_and_status_conditioned_conflicting_repeated_contexts: 55,
      complete_packet_context_repeated_control_witnesses: 0,
      packet_context_fields_audited_leave_one_out: 18,
      sole_event_fragmenting_packet_field:
        "skill_effect_component_index-container-ordinal-not-formula-authority",
      group_relative_same_capture_target_lifecycle_conflicting_contexts: 55,
      reconstructed_pre_hit_hp_available_conflicting_observations: 220,
      reconstructed_pre_hit_hp_repeated_control_witnesses: 0,
      strict_controlled_damage_pairs: 0,
      affected_hit_rows_selected: false,
      integer_damage_counterfactual_complete: false,
      current_build_conservation_replay_complete: false,
      runtime_rdps_enabled: false,
    },
    current_runtime_summary: compactRuntimeSummary(superconductorStatsRuntime),
    remaining_proof_obligations: [
      "replay each recipient's authoritative pre-effect attribute snapshot",
      "select the equipped tier from that run's frozen Imagine loadout",
      "resolve the one same-packet attack-percent confounder and prove the complete primary-to-attack marginal for every retained boundary",
      "capture or otherwise prove event-time semantics for missing Attack-family attributes 11035, 11333, and 11334; never zero-fill their absence",
      "obtain damage pairs with every other packet-observed source/target status and action input held constant",
      "keep status-presence-only rows outside the eight exact recipient stat windows ambiguous; the gated four-session audit retains 12,547 affected actions, reclassifies 1,841 unsupported rows, and leaves zero stable-source controlled inactive/active overlaps",
      "retain the allegiance-neutral mitigation action topology: all 774 defense-bearing targets are packet-observed player actors, 773 damage actors and 764 run scenes are event-time joined, but exact state controls leave zero groups with multiple defense values and therefore prove none of the 6500, 22000, or 9980 curve candidates",
      "execute the exact controlled-replay acquisition contract for physical, magical, refined, and elemental mitigation: the current-build client search finds only generated constants and two character-sheet UI consumers, no authoritative combat consumer, while 774 ranked exact actions contain zero isolated mitigation-axis pairs; accept only a deterministic divergent pair that uniquely selects an integer model, repeats its rounding, and conserves packet damage",
      "prove the actual multi-stage downstream pipeline for DamageAttr 2220352105; all 3,840 enumerated orders/rounding assignments for exact attributes 12510, 11940, 12550, and 13170 plus one unresolved integer factor reject at least 11,105 of 14,810 eligible observations, and all five optional-attribute presence partitions still have zero complete candidates",
      "resolve the remaining server damage operator and obtain repeated controlled witnesses: exact lifecycle context leaves 152 conflicting repeated contexts, target identity leaves 141, and complete retained attribute/status IDs leave 55; adding full packet calculation context fragments all 2,406 joined observations into event-unique contexts, which removes every repeated witness and is not formula proof",
      "do not treat SkillEffect component index as a damage multiplier: it is the only packet-context field that fragments the 55 repeated conflicts, but output rises and falls equally across component order and the index is container position rather than a proven server formula input",
      "retain the exact target-delta and damage-array topology without treating either index as arithmetic; packet-wide counts and same-capture source/target lifecycle signatures leave all 55 conflicts intact",
      "acquire repeated equal-pre-hit-HP controls or a proven server consumer before using CurrentHP: bounded reconstruction distinguishes 220 conflict observations but leaves every valid row event-unique, 20 reconstructions are impossible negative values, and 23 rows lack the required HP fields",
      "replay only hit rows inside the proven effect 2110140 lifecycle",
      "prove recipient debit equals provider credit for the selected run segment",
    ],
  },
  {
    imagine_skill_id: 3971,
    imagine_name: superconductorStatic.imagine_name_evidence,
    component_id: "superconductor-surge-mechanical-power-healing-received",
    effect_ids: [2110140],
    proof_state: allSuperconductorTablesExact
      ? "current-build-tier-parameter-and-external-lifecycle-exact"
      : "current-build-formula-proof-incomplete",
    exact_component_scalar_available: allSuperconductorTablesExact,
    matching_build_external_lifecycle_observed:
      Number(superconductorHealingRuntime.summary?.external_player_status_rows ?? 0) > 0,
    base_parameter_pair: superconductorStatic.base_parameter_pair,
    tier_parameter_pairs: superconductorStatic.tier_parameter_pairs,
    loadout_tier_parameter_pairs: superconductorStatic.loadout_tier_parameter_pairs,
    star_increment_parameter_pairs: superconductorStatic.star_increment_parameter_pairs,
    duration_millis: 15000,
    attribution_contract: {
      lane: "healing-only",
      damage_credit_allowed: false,
      effective_healing_counterfactual_complete: false,
      overheal_replay_complete: false,
      current_build_conservation_replay_complete: false,
      runtime_rdps_enabled: false,
    },
    current_runtime_summary: compactRuntimeSummary(superconductorHealingRuntime),
    remaining_proof_obligations: [
      "replay effective healing and overheal separately inside the effect lifecycle",
      "keep all resulting attribution out of the damage rDPS lane",
      "prove recipient healing debit equals provider healing credit",
    ],
  },
];

const report = {
  schema_version: 1,
  generated_by: "tools/rdps-imagine-formula-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: gameBuild,
  policy: {
    exact_current_build_only: true,
    run_owned_equipped_imagine_identity_and_tier_are_authoritative: true,
    later_profile_snapshots_never_rewrite_historical_runs: true,
    direct_summon_damage_remains_owner_damage: true,
    static_scalar_or_native_equation_does_not_enable_rdps_without_event_replay: true,
    external_lifecycle_is_required_but_not_sufficient: true,
    recipient_debit_must_equal_provider_credit: true,
    unresolved_evidence_is_never_hidden: true,
    effect_recipient_and_damage_action_target_allegiance_are_never_assumed: true,
  },
  inputs: {
    client_binary_identity: receipt(currentIdentityPath),
    game_assembly: assemblyFingerprint,
    il2cpp_dump: receipt(dumpPath),
    matching_build_runtime_proof: receipt(runtimeProofPath),
    primary_stat_transform_proof: receipt(primaryTransformPath),
    primary_attack_runtime_route_proof: receipt(primaryRuntimePath),
    primary_attack_packet_marginal_proof: primaryAttackTransitionPath
      ? receipt(primaryAttackTransitionPath)
      : null,
    attack_counterfactual_audits: attackCounterfactualPaths.map(receipt),
    attack_action_wide_downstream_diagnostic: attackDownstreamDiagnosticPath
      ? receipt(attackDownstreamDiagnosticPath)
      : null,
    attack_source_stage_order_proof: attackSourceStageOrderProofPath
      ? receipt(attackSourceStageOrderProofPath)
      : null,
    attack_lifecycle_ancestry_proof: attackLifecycleAncestryProofPath
      ? receipt(attackLifecycleAncestryProofPath)
      : null,
    attack_lifecycle_conditioned_diagnostic: attackLifecycleConditionedDiagnosticPath
      ? receipt(attackLifecycleConditionedDiagnosticPath)
      : null,
    attack_group_relative_topology_diagnostic: attackGroupRelativeDiagnosticPath
      ? receipt(attackGroupRelativeDiagnosticPath)
      : null,
    attack_current_hp_diagnostic: attackCurrentHpDiagnosticPath
      ? receipt(attackCurrentHpDiagnosticPath)
      : null,
    attack_interval_closed_hp_diagnostic: attackIntervalClosedHpDiagnosticPath
      ? receipt(attackIntervalClosedHpDiagnosticPath)
      : null,
    attack_target_static_context_diagnostic: attackTargetStaticContextDiagnosticPath
      ? receipt(attackTargetStaticContextDiagnosticPath)
      : null,
    attack_effective_stat_window_damage_diagnostic:
      attackEffectiveStatWindowDiagnosticPath
        ? receipt(attackEffectiveStatWindowDiagnosticPath)
        : null,
    attack_target_mitigation_action_identity:
      attackTargetMitigationActionIdentityPath
        ? receipt(attackTargetMitigationActionIdentityPath)
        : null,
    attack_target_mitigation_diagnostic: attackTargetMitigationDiagnosticPath
      ? receipt(attackTargetMitigationDiagnosticPath)
      : null,
    attack_target_mitigation_offline_exhaustion:
      attackTargetMitigationOfflineExhaustionPath
        ? receipt(attackTargetMitigationOfflineExhaustionPath)
        : null,
    attack_target_mitigation_controlled_replay:
      attackTargetMitigationControlledReplayPath
        ? receipt(attackTargetMitigationControlledReplayPath)
        : null,
    current_skill_fight_level_table: receipt(
      path.join(repoRoot, "Excels", "SkillFightLevelTable.json"),
    ),
    current_skill_aoyi_table: receipt(path.join(repoRoot, "Excels", "SkillAoyiTable.json")),
    current_skill_aoyi_star_table: receipt(
      path.join(repoRoot, "Excels", "SkillAoyiStarTable.json"),
    ),
    current_skill_table: receipt(path.join(repoRoot, "Excels", "SkillTable.json")),
    current_skill_effect_table: receipt(path.join(repoRoot, "Excels", "SkillEffectTable.json")),
    current_buff_table: receipt(path.join(repoRoot, "Excels", "BuffTable.json")),
    historical_time_decree_proof: receipt(timeDecreePriorPath),
    historical_superconductor_proof: receipt(superconductorPriorPath),
    fatal_spiral_temp_attribute_audit: receipt(fatalSpiralTempAttributePath),
    fatal_spiral_all_element_family_proof: receipt(fatalSpiralFamilyPath),
    fatal_spiral_status_attribute_correlation: receipt(fatalSpiralCorrelationPath),
  },
  static_dependency_proof: tableProofs,
  native_cooldown_equation_proof: {
    function_rva: "0x5391ff0",
    signature_rva: `0x${cooldownSignatureRva.toString(16)}`,
    equation:
      "Progress = InitProgress + (Now - CdBeginTime) * (1 + CdAccelerateRate) / CdRealLen",
    signature: cooldownNativeProof,
    dump_contracts: dumpProof.contracts,
    exact_current_build_equation: cooldownEquationExact,
  },
  summary: {
    component_proofs: components.length,
    components_with_exact_current_scalar: components.filter(
      (component) => component.exact_component_scalar_available,
    ).length,
    components_with_matching_build_external_lifecycle: components.filter(
      (component) => component.matching_build_external_lifecycle_observed,
    ).length,
    offensive_components_runtime_enabled: components.filter(
      (component) => component.attribution_contract.runtime_rdps_enabled === true,
    ).length,
    components_requiring_conservation_replay: components.filter(
      (component) =>
        component.attribution_contract.current_build_conservation_replay_complete !== true,
    ).length,
    direct_summon_damage_transferred_to_support_rdps: 0,
  },
  components,
};

fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({ output: relative(outputPath), summary: report.summary }, null, 2));

function verifyPrimaryAttackPacketMarginal(proof, build) {
  if (!proof) return null;
  if (Number(proof.schema_version) !== 1 ||
    proof.generated_by !== "tools/rdps-imagine-primary-attack-transition-proof.mjs" ||
    String(proof.game_build) !== String(build) || Number(proof.effect_id) !== 2110140 ||
    proof.topology?.effect_edge !==
      "provider -> effect/status lifecycle -> recipient or enemy target" ||
    proof.topology?.damage_edge !==
      "recipient damage action -> recipient or enemy target" ||
    proof.topology?.allegiance_assumptions !== false ||
    proof.policy?.effective_stat_window_uses_exact_attribute_transition_boundaries !== true ||
    proof.policy?.integer_damage_stage_order_and_rounding_proven !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(proof.packet_transform_proof?.exact_boundary_count) !== 16 ||
    Number(proof.packet_transform_proof?.exact_58_over_100_matches) !== 15 ||
    Number(proof.packet_transform_proof?.unresolved_same_packet_confounders) !== 1 ||
    Number(proof.packet_transform_proof?.static_1_over_8_matches) !== 0 ||
    proof.packet_transform_proof?.current_formula_artifact_requires_correction !== true ||
    Number(proof.summary?.exact_lifecycle_windows) !== 8 ||
    Number(proof.summary?.unique_attribute_boundary_joins) !== 16 ||
    Number(proof.summary?.effective_stat_window_damage_actions) !== 12547 ||
    Number(proof.summary?.excluded_before_attribute_activation) !== 10 ||
    String(proof.summary?.observed_damage_reassigned_to_provider) !== "0") {
    throw new Error("Current-build primary/attack packet marginal proof is unsafe or incomplete");
  }
  return {
    proof: receipt(primaryAttackTransitionPath),
    class_id: 11,
    effect_id: 2110140,
    primary_current_attribute_id: 11030,
    primary_total_attribute_id: 11031,
    primary_percent_attribute_id: 11034,
    attack_add_attribute_id: 11332,
    expression:
      "delta(attack_add_11332) = floor(after_primary_current_11030 * 58 / 100) - floor(before_primary_current_11030 * 58 / 100)",
    rounding: "floor on each nonnegative packet state before subtraction",
    exact_lifecycle_windows: 8,
    exact_transition_boundaries: 16,
    exact_expression_matches: 15,
    unresolved_same_packet_attack_percent_confounders: 1,
    effective_stat_window_damage_actions: 12547,
    static_1_over_8_complete_packet_marginal_matches: 0,
    complete_packet_marginal_proven_for_every_boundary: false,
    integer_damage_counterfactual_complete: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackCounterfactualAudits(proofs, build) {
  const expected = [
    {
      session_id: "monitor-1787002076016.run-0004",
      final_attack_delta: 346,
      events: 131,
      observed_damage: "20259776",
      integer_factor_intervals: 68,
      one_interval_counterfactual: 65,
    },
    {
      session_id: "monitor-1787002076016.run-0010",
      final_attack_delta: 345,
      events: 2,
      observed_damage: "183616",
      integer_factor_intervals: 2,
      one_interval_counterfactual: 2,
    },
  ];
  if (!Array.isArray(proofs) || proofs.length !== expected.length) {
    throw new Error("Current-build event-time Attack counterfactual audit set is incomplete");
  }
  const ordered = proofs.slice().sort((left, right) =>
    String(left.sessions?.[0]?.session_id).localeCompare(String(right.sessions?.[0]?.session_id))
  );
  for (let index = 0; index < expected.length; index += 1) {
    const proof = ordered[index];
    const wanted = expected[index];
    const single = proof.single_event_damage_attr_counterfactual;
    const coverage = single?.attack_family_component_coverage ?? [];
    const buckets = single?.exact_conserved_share_buckets ?? [];
    const bucketEvents = buckets.reduce((sum, entry) => sum + Number(entry.events ?? 0), 0);
    if (Number(proof.schema_version) !== 46 ||
      proof.generated_by !== "rlogs-bpsr-external-attack-damage-proof" ||
      String(proof.sessions?.[0]?.session_id) !== wanted.session_id ||
      (proof.sessions ?? []).length !== 1 ||
      String(proof.damage_surface_identity?.game_build) !== String(build) ||
      proof.damage_surface_identity?.build_identity_verified !== true ||
      Number(proof.selected_effect_id) !== 2110140 ||
      Number(proof.selected_source_config_id) !== 0 ||
      proof.selected_source_config_is_absent !== true ||
      Number(proof.source_entity_uuid_filter) !== 216009015936 ||
      proof.pair_proof_only !== false ||
      Number(proof.transition_seed_selection?.retained_transition_seeds) !== 1 ||
      proof.transition_seed_selection?.formula_authority !== false ||
      proof.attack_provider_delta?.component !== "final_attack" ||
      Number(proof.attack_provider_delta?.raw_delta) !== wanted.final_attack_delta ||
      proof.policy?.unresolved_evidence_is_hidden !== false ||
      Number(proof.formula?.pairs) !== 0 ||
      (proof.pair_groups ?? []).length !== 0 ||
      Number(single?.external_active_damage_events) !== wanted.events ||
      Number(single?.events_with_all_required_attack_family_components) !== wanted.events ||
      Number(single?.events_with_exact_attack_family_reversal) !== wanted.events ||
      Number(single?.events_without_exact_attack_family_reversal) !== 0 ||
      JSON.stringify(coverage.map((entry) => [
        Number(entry.attribute_id),
        Number(entry.events_present),
        Number(entry.events_missing),
      ])) !== JSON.stringify([[11330, wanted.events, 0]]) ||
      Number(single?.events_with_ability_hit_identity) !== wanted.events ||
      Number(single?.events_with_unique_damage_row) !== wanted.events ||
      Number(single?.events_with_matching_damage_script) !== wanted.events ||
      Number(single?.events_with_exact_stage_coefficient) !== wanted.events ||
      Number(single?.events_with_base_candidates) !== wanted.events ||
      Number(single?.events_with_exact_conserved_attack_stage_share) !== wanted.events ||
      String(single?.exact_conserved_share_observed_damage) !== wanted.observed_damage ||
      bucketEvents !== wanted.events ||
      buckets.some((entry) => entry.conservation_identity_holds !== true) ||
      (single?.exact_conserved_share_coverage_gaps ?? []).length !== 0 ||
      Number(single?.events_with_integer_post_base_factor_interval) !==
        wanted.integer_factor_intervals ||
      Number(single?.events_with_one_counterfactual_across_integer_factor_interval) !==
        wanted.one_interval_counterfactual ||
      Number(single?.events_with_one_exact_counterfactual_across_all_candidates) !== 0 ||
      Number(single?.events_with_ambiguous_counterfactual) !== wanted.events ||
      Number(single?.events_with_invalid_counterfactual) !== 0 ||
      String(single?.observed_damage) !== wanted.observed_damage ||
      String(single?.exact_counterfactual_damage) !== "0" ||
      String(single?.exact_provider_marginal) !== "0") {
      throw new Error(
        `Current-build event-time Attack counterfactual audit ${wanted.session_id} is unsafe or incomplete`,
      );
    }
  }
  return {
    proofs: attackCounterfactualPaths.map(receipt),
    effect_id: 2110140,
    recipient_entity_uuid: 216009015936,
    exact_tier: 0,
    exact_lifecycle_transition_seeds: 2,
    selected_damage_actions: 133,
    selected_reported_damage: "20443392",
    strict_controlled_damage_pairs: 0,
    events_with_exact_occurrence_scoped_final_attack_reversal: 133,
    events_with_unique_current_build_damage_row_and_stage: 133,
    events_with_exact_conserved_attack_stage_share: 133,
    events_with_integer_post_base_factor_interval: 70,
    events_with_one_counterfactual_across_integer_factor_interval: 67,
    events_with_one_exact_counterfactual_across_all_candidates: 0,
    exact_counterfactual_damage: "0",
    exact_provider_marginal: "0",
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackDownstreamDiagnostic(proof, build) {
  if (!proof) return null;
  const axes = Object.values(proof.axes ?? {});
  const diagnostic = proof.post_base_integer_factor_diagnostic;
  const contextRows = diagnostic?.factor_compatibility_by_calculation_context ?? [];
  const ownerStageRows = diagnostic?.factor_compatibility_by_owner_stage ?? [];
  const completeState = diagnostic?.factor_compatibility_by_complete_retained_state;
  const targetActorIdentity = diagnostic?.factor_compatibility_by_target_actor_identity;
  const criticalStage = diagnostic?.critical_damage_stage_diagnostic;
  const criticalBefore = Object.fromEntries(
    (criticalStage?.critical_stage_before_unknown_factor_candidates ?? [])
      .map((row) => [row.name, row.counters]),
  );
  const criticalAfter = Object.fromEntries(
    (criticalStage?.critical_stage_after_unknown_factor_candidates ?? [])
      .map((row) => [row.name, row.counters]),
  );
  const contextRowClasses = contextRows.reduce(
    (summary, row) => {
      const compatible = Number(
        row.counters?.samples_with_integer_factor_interval ?? 0,
      );
      const rejected = Number(
        row.counters?.samples_without_integer_factor_interval ?? 0,
      );
      if (compatible > 0 && rejected > 0) summary.mixed += 1;
      else if (compatible > 0) summary.only_compatible += 1;
      else if (rejected > 0) summary.only_rejected += 1;
      return summary;
    },
    { mixed: 0, only_compatible: 0, only_rejected: 0 },
  );
  const providerLikeContext = contextRows
    .filter((row) =>
      row.context?.critical === true &&
      row.context?.lucky === false &&
      Number(row.context?.property) === 7)
    .reduce(
      (summary, row) => {
        summary.samples += Number(
          row.counters?.samples_with_positive_base_and_output ?? 0,
        );
        summary.compatible += Number(
          row.counters?.samples_with_integer_factor_interval ?? 0,
        );
        summary.rejected += Number(
          row.counters?.samples_without_integer_factor_interval ?? 0,
        );
        return summary;
      },
      { samples: 0, compatible: 0, rejected: 0 },
    );
  const soleOwnerStage = ownerStageRows[0];
  if (Number(proof.schema_version) !== 2 ||
    proof.generated_by !==
      "rlogs-bpsr-target-mitigation-transform-proof:selected-ability-diagnostic" ||
    String(proof.game_build) !== String(build) ||
    JSON.stringify(proof.selection?.ability_ids) !== JSON.stringify([2203521]) ||
    Number(proof.selection?.hit_event_id) !== 5 ||
    Number(proof.selection?.coefficient_basis_points) !== 20000 ||
    Number(proof.selection?.selected_sample_count) !== 19858 ||
    Number(proof.selection?.samples_by_ability_id?.["2203521"]) !== 19858 ||
    proof.policy?.remote_player_only_packets_are_required !== false ||
    proof.policy?.remote_player_only_packets_are_synthesized !== false ||
    proof.policy?.remote_player_only_packets_are_treated_as_zero !== false ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.runtime_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    axes.length !== 12 ||
    axes.some((axis) => Number(axis.counters?.samples_with_axis) !== 0) ||
    Number(diagnostic?.source_attack_attribute_id) !== 11330 ||
    Number(diagnostic?.coefficient_basis_points) !== 20000 ||
    Number(diagnostic?.selected_samples) !== 19858 ||
    Number(diagnostic?.samples_with_source_attack) !== 16518 ||
    Number(diagnostic?.samples_with_positive_base_and_output) !== 16518 ||
    Number(diagnostic?.samples_with_integer_factor_interval) !== 10835 ||
    Number(diagnostic?.samples_without_integer_factor_interval) !== 5683 ||
    Number(diagnostic?.samples_with_unique_integer_factor) !== 10728 ||
    Number(diagnostic?.samples_where_normal_value_matches_amount) !== 19858 ||
    contextRows.length !== 4539 ||
    contextRowClasses.mixed !== 1285 ||
    contextRowClasses.only_compatible !== 2148 ||
    contextRowClasses.only_rejected !== 1106 ||
    providerLikeContext.samples !== 14839 ||
    providerLikeContext.compatible !== 9793 ||
    providerLikeContext.rejected !== 5046 ||
    ownerStageRows.length !== 1 ||
    Number(soleOwnerStage?.context?.owner_level) !== 1 ||
    soleOwnerStage?.context?.owner_stage !== null ||
    Number(soleOwnerStage?.counters?.samples_with_integer_factor_interval) !== 10835 ||
    Number(soleOwnerStage?.counters?.samples_without_integer_factor_interval) !== 5683 ||
    Number(completeState?.complete_retained_state_rows) !== 16515 ||
    Number(completeState?.mixed_compatible_and_rejected_rows) !== 0 ||
    Number(completeState?.only_compatible_rows) !== 10833 ||
    Number(completeState?.only_rejected_rows) !== 5682 ||
    Number(completeState?.samples_in_mixed_compatible_and_rejected_rows) !== 0 ||
    completeState?.formula_authority !== false ||
    Number(targetActorIdentity?.eligible_samples_without_target_actor_identity) !== 16518 ||
    (targetActorIdentity?.rows ?? []).length !== 0 ||
    targetActorIdentity?.formula_authority !== false ||
    Number(criticalStage?.critical_damage_attribute_id) !== 12510 ||
    criticalStage?.missing_remote_critical_damage_is_not_zero !== true ||
    Number(criticalStage?.critical_true_samples_with_source_attack) !== 14839 ||
    Number(criticalStage?.critical_true_samples_with_critical_damage) !== 14839 ||
    Number(criticalStage?.critical_true_samples_without_critical_damage) !== 0 ||
    Number(criticalBefore.additive_bonus_floor?.samples_with_integer_other_factor_interval) !== 3229 ||
    Number(criticalBefore.additive_bonus_floor?.samples_without_integer_other_factor_interval) !== 11610 ||
    Number(criticalBefore.additive_bonus_half_up?.samples_with_integer_other_factor_interval) !== 2982 ||
    Number(criticalBefore.additive_bonus_half_up?.samples_without_integer_other_factor_interval) !== 11857 ||
    Number(criticalBefore.direct_total_floor?.samples_with_integer_other_factor_interval) !== 4976 ||
    Number(criticalBefore.direct_total_floor?.samples_without_integer_other_factor_interval) !== 9863 ||
    Number(criticalBefore.direct_total_half_up?.samples_with_integer_other_factor_interval) !== 5023 ||
    Number(criticalBefore.direct_total_half_up?.samples_without_integer_other_factor_interval) !== 9816 ||
    Number(criticalAfter.additive_bonus_floor?.samples_with_integer_other_factor_interval) !== 3548 ||
    Number(criticalAfter.additive_bonus_floor?.samples_without_integer_other_factor_interval) !== 1490 ||
    Number(criticalAfter.additive_bonus_half_up?.samples_with_integer_other_factor_interval) !== 3189 ||
    Number(criticalAfter.additive_bonus_half_up?.samples_without_integer_other_factor_interval) !== 1380 ||
    Number(criticalAfter.direct_total_floor?.samples_with_integer_other_factor_interval) !== 4880 ||
    Number(criticalAfter.direct_total_floor?.samples_without_integer_other_factor_interval) !== 2436 ||
    Number(criticalAfter.direct_total_half_up?.samples_with_integer_other_factor_interval) !== 4587 ||
    Number(criticalAfter.direct_total_half_up?.samples_without_integer_other_factor_interval) !== 2648 ||
    criticalStage?.formula_authority !== false ||
    diagnostic?.formula_authority !== false ||
    diagnostic?.runtime_authority !== false ||
    diagnostic?.provider_rdps_credit_allowed !== false) {
    throw new Error("Current-build Attack downstream diagnostic is unsafe or incomplete");
  }
  return {
    proof: receipt(attackDownstreamDiagnosticPath),
    ability_id: 2203521,
    hit_event_id: 5,
    damage_attr_id: 2220352105,
    coefficient_basis_points: 20000,
    selected_samples: 19858,
    samples_with_source_attack: 16518,
    samples_with_integer_post_base_factor_interval: 10835,
    samples_rejecting_one_integer_post_base_factor: 5683,
    observed_calculation_context_rows: 4539,
    mixed_compatibility_context_rows: 1285,
    provider_like_observed_flag_samples: providerLikeContext.samples,
    provider_like_observed_flag_rejections: providerLikeContext.rejected,
    observed_owner_stage_contexts: ownerStageRows.map((row) => ({
      ...row.context,
      ...row.counters,
    })),
    complete_retained_state_rows: 16515,
    mixed_complete_retained_state_rows: 0,
    eligible_samples_without_target_actor_identity: 16518,
    game_file_target_config_join_available: false,
    critical_damage_attribute_id: 12510,
    critical_true_samples_with_critical_damage: 14839,
    critical_stage_candidate_results: {
      before_unknown_factor: criticalBefore,
      after_unknown_factor: criticalAfter,
    },
    target_mitigation_axes_observed: 0,
    conclusion:
      "the exact action family disproves a universal one-integer-post-base-factor pipeline; complete retained states are nearly event-unique rather than controlled repeats, every eligible event lacks a target actor identity for a game-file target-config join, and neither tested position of attribute 12510 under additive/direct plus floor/half-up completes a two-stage pipeline, so compatible provider-window events remain diagnostic and cannot be promoted by extrapolation",
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackSourceStageOrderProof(proof, diagnosticPath, build) {
  if (!proof || !diagnosticPath) return null;
  const diagnosticReceipt = receipt(diagnosticPath);
  const unsignedProof = { ...proof };
  delete unsignedProof.content_sha256;
  const expectedContentSha256 = crypto
    .createHash("sha256")
    .update(JSON.stringify(unsignedProof))
    .digest("hex");
  const best = proof.best_candidates?.[0];
  const expectedPresencePartitions = [
    { ids: [], observations: 5948, minimum_rejections: 3860 },
    { ids: [13100], observations: 4529, minimum_rejections: 3240 },
    { ids: [11840, 11950], observations: 3660, minimum_rejections: 2782 },
    { ids: [11840, 11950, 13100], observations: 592, minimum_rejections: 400 },
    { ids: [12670, 13100], observations: 81, minimum_rejections: 50 },
  ];
  const actualPresencePartitions =
    (proof.summary?.optional_attribute_presence_partitions ?? []).map((row) => ({
      ids: row.present_optional_attribute_ids,
      observations: Number(row.observations),
      minimum_rejections: Number(row.minimum_rejections),
    }));
  if (Number(proof.schema_version) !== 1 ||
    proof.generated_by !== "tools/bpsr-source-stage-order-proof.mjs" ||
    String(proof.game_build) !== String(build) ||
    Number(proof.selection?.ability_id) !== 2203521 ||
    Number(proof.selection?.hit_event_id) !== 5 ||
    Number(proof.selection?.damage_attr_id) !== 2220352105 ||
    Number(proof.selection?.coefficient_basis_points) !== 20000 ||
    proof.policy?.missing_attributes_are_not_zero !== true ||
    proof.policy?.remote_player_only_packets_are_required !== false ||
    proof.policy?.remote_player_only_packets_are_synthesized !== false ||
    proof.policy?.target_actor_allegiance_is_inferred !== false ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.runtime_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    proof.input?.sha256 !== diagnosticReceipt.sha256 ||
    Number(proof.input?.bytes) !== diagnosticReceipt.bytes ||
    JSON.stringify(proof.model_space?.known_stage_attribute_ids) !==
      JSON.stringify([12510, 11940, 12550, 13170]) ||
    JSON.stringify(proof.model_space?.retained_candidate_stage_attribute_ids) !==
      JSON.stringify([11840, 11880, 11940, 11950, 12510, 12550, 12590, 12610,
        12630, 12670, 12690, 12710, 12730, 13100, 13170]) ||
    Number(proof.model_space?.known_stage_orders) !== 24 ||
    Number(proof.model_space?.known_stage_rounding_assignments) !== 16 ||
    Number(proof.model_space?.critical_interpretations) !== 2 ||
    Number(proof.model_space?.unknown_integer_factor_positions) !== 5 ||
    Number(proof.model_space?.candidate_models) !== 3840 ||
    Number(proof.summary?.observations) !== 14810 ||
    Number(proof.summary?.zero_rejection_candidates) !== 0 ||
    Number(proof.summary?.minimum_rejections) !== 11105 ||
    Number(proof.summary?.maximum_compatible_observations) !== 3705 ||
    Number(proof.summary?.maximum_unique_factor_observations) !== 3694 ||
    JSON.stringify(actualPresencePartitions) !== JSON.stringify(expectedPresencePartitions) ||
    (proof.summary?.optional_attribute_presence_partitions ?? []).some(
      (row) => Number(row.zero_rejection_candidates) !== 0,
    ) ||
    Number(best?.counters?.observations) !== 14810 ||
    Number(best?.counters?.compatible) !== 3705 ||
    Number(best?.counters?.rejected) !== 11105 ||
    best?.formula_authority !== false ||
    best?.runtime_authority !== false ||
    best?.provider_rdps_credit_allowed !== false ||
    proof.formula_authority !== false ||
    proof.runtime_authority !== false ||
    proof.provider_rdps_credit_allowed !== false ||
    proof.content_sha256 !== expectedContentSha256) {
    throw new Error("Current-build Attack source-stage order proof is unsafe or incomplete");
  }
  return {
    proof: receipt(attackSourceStageOrderProofPath),
    observation_input: diagnosticReceipt,
    damage_attr_id: 2220352105,
    known_stage_attribute_ids: [12510, 11940, 12550, 13170],
    observations: 14810,
    candidate_models: 3840,
    zero_rejection_candidates: 0,
    minimum_rejections: 11105,
    maximum_compatible_observations: 3705,
    optional_attribute_presence_partitions: expectedPresencePartitions.map((row) => ({
      present_optional_attribute_ids: row.ids,
      observations: row.observations,
      zero_rejection_candidates: 0,
      minimum_rejections: row.minimum_rejections,
    })),
    conclusion:
      "the exhaustive four-stage plus one-unresolved-factor model family is disproven for this exact action cohort; no ranked candidate is formula or runtime authority",
    effect_recipient_allegiance_inferred: false,
    damage_action_target_allegiance_inferred: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackLifecycleAncestryProof(proof, build) {
  if (!proof) return null;
  const summary = proof.summary;
  const proximity = proof.diagnostic_proximity?.nearest_preceding_transition;
  const consumed = proof.diagnostic_proximity?.nearest_preceding_consumed_transition;
  if (Number(proof.schema_version) !== 1 ||
    proof.generated_by !== "rlogs-bpsr-buff-lifecycle-damage-ancestry-proof" ||
    String(proof.game_build) !== String(build) ||
    Number(proof.selection?.effect_id) !== 2203521 ||
    Number(proof.selection?.action_id) !== 2203521 ||
    Number(proof.selection?.exact_damage_surface?.hit_event_id) !== 5 ||
    Number(proof.selection?.exact_damage_surface?.damage_source) !== 2 ||
    Number(proof.selection?.exact_damage_surface?.property) !== 7 ||
    Number(proof.selection?.exact_damage_surface?.owner_id) !== 2203521 ||
    Number(proof.input?.timeline_schema_version) !== 10 ||
    Number(proof.input?.declared_rlog_count) !== 4 ||
    Number(proof.input?.observed_run_header_count) !== 4 ||
    JSON.stringify(proof.input?.client_builds) !== JSON.stringify([String(build)]) ||
    proof.policy?.affected_entity_allegiance_is_assumed !== false ||
    proof.policy?.damage_target_allegiance_is_assumed !== false ||
    proof.policy?.remote_player_cast_packets_required !== false ||
    proof.policy?.remote_player_cast_packets_synthesized !== false ||
    proof.policy?.missing_cast_packets_treated_as_zero !== false ||
    proof.policy?.proximity_grants_causal_ancestry !== false ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.runtime_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(summary?.selected_effect_transition_count) !== 7561 ||
    Number(summary?.selected_action_row_count) !== 2709 ||
    Number(summary?.exact_damage_surface_count) !== 2709 ||
    Number(summary?.exact_damage_surface_with_preceding_matching_endpoint_count) !== 2709 ||
    Number(summary?.exact_damage_surface_with_preceding_consumed_matching_endpoint_count) !== 2618 ||
    JSON.stringify(summary?.relationship_counts) !== JSON.stringify([
      { key: "affected-entity-equals-damage-target", count: 2709 },
    ]) ||
    JSON.stringify(summary?.provider_equals_damage_actor_counts) !== JSON.stringify([
      { key: "true", count: 2709 },
    ]) ||
    JSON.stringify(summary?.selected_source_config_counts) !== JSON.stringify([
      { key: "2203520", count: 3966 },
      { key: "2203620", count: 1092 },
      { key: "2203670", count: 2503 },
    ]) ||
    Number(proximity?.bands?.same_capture_sequence) !== 1661 ||
    Number(proximity?.bands?.within_1000_millis) !== 2706 ||
    Number(consumed?.bands?.same_capture_sequence) !== 915 ||
    Number(consumed?.bands?.unmatched) !== 91 ||
    !Array.isArray(proof.exact_damage_surface_receipts) ||
    proof.exact_damage_surface_receipts.length !== 2709 ||
    proof.conclusion?.causal_ancestry_proven !== false ||
    proof.conclusion?.exact_damage_formula_proven !== false ||
    proof.conclusion?.provider_rdps_credit_allowed !== false) {
    throw new Error("Current-build Attack lifecycle ancestry proof is unsafe or incomplete");
  }
  return {
    proof: receipt(attackLifecycleAncestryProofPath),
    effect_id: 2203521,
    action_id: 2203521,
    selected_effect_transitions: 7561,
    exact_damage_surface_rows: 2709,
    rows_with_preceding_matching_effect_endpoint: 2709,
    observed_relationship: "affected-entity-equals-damage-target",
    rows_where_effect_provider_equals_damage_actor: 2709,
    observed_source_config_ids: [2203520, 2203620, 2203670],
    rows_with_same_capture_sequence_transition: 1661,
    relationship_receipt_available: true,
    causal_ancestry_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackLifecycleConditionedDiagnostic(
  proof,
  diagnosticPath,
  ancestryPath,
  build,
) {
  if (!proof || !diagnosticPath || !ancestryPath) return null;
  const diagnosticReceipt = receipt(diagnosticPath);
  const ancestryReceipt = receipt(ancestryPath);
  const summary = proof.summary;
  const sourceOnly = proof.context_diagnostics?.source_stage_vector_only;
  const lifecycle = proof.context_diagnostics?.source_stage_vector_plus_lifecycle;
  const lifecycleTarget =
    proof.context_diagnostics?.source_stage_vector_lifecycle_and_target_identity;
  const lifecycleTargetStatus =
    proof.context_diagnostics?.source_stage_vector_lifecycle_target_and_status_state_ids;
  const completeState = proof.context_diagnostics?.complete_retained_state_ids_plus_lifecycle;
  const completePacket =
    proof.context_diagnostics?.complete_retained_state_and_packet_calculation_context;
  const packetFields = proof.packet_context_field_diagnostics;
  const conflictingContexts = proof.conflicting_context_diagnostics;
  if (Number(proof.schema_version) !== 1 ||
    proof.generated_by !== "rlogs-bpsr-lifecycle-conditioned-damage-observations" ||
    String(proof.game_build) !== String(build) ||
    Number(proof.selection?.damage_attr_id) !== 2220352105 ||
    Number(proof.selection?.action_id) !== 2203521 ||
    Number(proof.selection?.hit_event_id) !== 5 ||
    Number(proof.selection?.coefficient_basis_points) !== 20000 ||
    Number(proof.selection?.effect_id) !== 2203521 ||
    proof.inputs?.selected_action_diagnostic?.sha256 !== diagnosticReceipt.sha256 ||
    Number(proof.inputs?.selected_action_diagnostic?.bytes) !== diagnosticReceipt.bytes ||
    proof.inputs?.lifecycle_damage_ancestry_proof?.sha256 !== ancestryReceipt.sha256 ||
    Number(proof.inputs?.lifecycle_damage_ancestry_proof?.bytes) !== ancestryReceipt.bytes ||
    proof.policy?.affected_entity_and_damage_target_allegiance_are_not_assumed !== true ||
    proof.policy?.missing_lifecycle_receipts_are_omitted_not_zero_filled !== true ||
    proof.policy?.lifecycle_proximity_grants_causal_ancestry !== false ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.runtime_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(summary?.source_stage_observation_count) !== 14810 ||
    Number(summary?.lifecycle_receipt_count) !== 2709 ||
    Number(summary?.exact_identity_join_count) !== 2406 ||
    Number(summary?.source_stage_observations_without_lifecycle_receipt_count) !== 12404 ||
    Number(summary?.lifecycle_receipts_without_source_stage_observation_count) !== 303 ||
    JSON.stringify(summary?.lifecycle_relationship_role_counts) !== JSON.stringify([
      { key: "affected-entity-equals-damage-target", count: 2406 },
    ]) ||
    JSON.stringify(summary?.lifecycle_provider_equals_damage_actor_counts) !==
      JSON.stringify([{ key: "true", count: 2406 }]) ||
    Number(sourceOnly?.conflicting_repeated_context_count) !== 169 ||
    Number(sourceOnly?.conflicting_repeated_observation_count) !== 2020 ||
    Number(sourceOnly?.maximum_distinct_outputs_in_one_context) !== 30 ||
    Number(lifecycle?.conflicting_repeated_context_count) !== 152 ||
    Number(lifecycle?.conflicting_repeated_observation_count) !== 649 ||
    Number(lifecycle?.maximum_distinct_outputs_in_one_context) !== 8 ||
    Number(lifecycleTarget?.conflicting_repeated_context_count) !== 141 ||
    Number(lifecycleTarget?.conflicting_repeated_observation_count) !== 426 ||
    Number(lifecycleTarget?.maximum_distinct_outputs_in_one_context) !== 3 ||
    Number(lifecycleTargetStatus?.conflicting_repeated_context_count) !== 55 ||
    Number(lifecycleTargetStatus?.conflicting_repeated_observation_count) !== 227 ||
    Number(completeState?.conflicting_repeated_context_count) !== 55 ||
    Number(completeState?.conflicting_repeated_observation_count) !== 227 ||
    Number(completePacket?.context_count) !== 2406 ||
    Number(completePacket?.repeated_context_count) !== 0 ||
    Number(completePacket?.conflicting_repeated_context_count) !== 0 ||
    Number(packetFields?.field_count) !== 18 ||
    JSON.stringify(proof.conclusion?.individually_discriminating_packet_context_fields) !==
      JSON.stringify(["calculation_context.skill_effect_component_index"]) ||
    JSON.stringify(proof.conclusion?.fields_whose_omission_restores_repeated_contexts) !==
      JSON.stringify(["calculation_context.skill_effect_component_index"]) ||
    Number(conflictingContexts?.conflicting_context_count) !== 55 ||
    Number(conflictingContexts?.contexts_where_every_component_index_is_distinct) !== 55 ||
    Number(conflictingContexts?.adjacent_output_delta_direction_by_component_index?.positive) !== 30 ||
    Number(conflictingContexts?.adjacent_output_delta_direction_by_component_index?.negative) !== 30 ||
    Number(conflictingContexts?.adjacent_output_delta_direction_by_component_index?.zero) !== 112 ||
    conflictingContexts?.component_index_formula_authority !== false ||
    !Array.isArray(proof.observations) ||
    proof.observations.length !== 2406 ||
    proof.conclusion?.lifecycle_context_eliminates_all_repeated_context_output_conflicts !== false ||
    proof.conclusion?.target_identity_eliminates_all_repeated_context_output_conflicts !== false ||
    proof.conclusion?.retained_status_state_ids_eliminate_all_repeated_context_output_conflicts !== false ||
    proof.conclusion?.complete_retained_state_ids_eliminate_all_repeated_context_output_conflicts !== false ||
    proof.conclusion?.packet_calculation_context_eliminates_all_repeated_context_output_conflicts !== true ||
    proof.conclusion?.packet_calculation_context_repeated_control_witnesses_available !== false ||
    proof.conclusion?.event_unique_context_fragmentation_is_formula_proof !== false ||
    proof.conclusion?.component_index_is_proven_damage_formula_input !== false ||
    proof.conclusion?.causal_lifecycle_to_damage_formula_proven !== false ||
    proof.conclusion?.exact_damage_formula_proven !== false ||
    proof.conclusion?.provider_rdps_credit_allowed !== false) {
    throw new Error(
      "Current-build lifecycle-conditioned damage diagnostic is unsafe or incomplete",
    );
  }
  return {
    proof: receipt(attackLifecycleConditionedDiagnosticPath),
    exact_identity_joined_observations: 2406,
    source_stage_only_conflicting_repeated_contexts: 169,
    lifecycle_conditioned_conflicting_repeated_contexts: 152,
    lifecycle_and_target_conditioned_conflicting_repeated_contexts: 141,
    lifecycle_conditioned_conflicting_observations: 649,
    lifecycle_and_target_conditioned_conflicting_observations: 426,
    lifecycle_target_and_status_conditioned_conflicting_contexts: 55,
    lifecycle_target_and_status_conditioned_conflicting_observations: 227,
    complete_retained_state_conditioned_conflicting_contexts: 55,
    complete_packet_context_repeated_control_witnesses: 0,
    packet_context_fields_audited_leave_one_out: 18,
    sole_event_fragmenting_packet_field:
      "calculation_context.skill_effect_component_index",
    component_index_adjacent_output_direction_counts: {
      positive: 30,
      negative: 30,
      zero: 112,
    },
    component_index_formula_authority: false,
    maximum_distinct_outputs_after_lifecycle_and_target_conditioning: 3,
    conclusion:
      "exact lifecycle and retained state context reduce ambiguity; only SkillEffect component index fragments the remaining repeats, but bidirectional output movement proves no monotonic component-order rule and container position is not formula authority; the server damage operator remains unresolved",
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackGroupRelativeDiagnostic(proof, build) {
  const summary = proof?.summary;
  const lifecycle = proof?.diagnostics?.same_capture_lifecycle;
  const availableFields = proof?.diagnostics?.group_relative_fields ?? [];
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "rlogs-bpsr-skill-effect-group-relative-diagnostic" ||
    String(proof?.game_build) !== String(build) ||
    Number(proof?.selection?.damage_attr_id) !== 2220352105 ||
    Number(proof?.selection?.action_id) !== 2203521 ||
    Number(summary?.selected_observation_count) !== 2406 ||
    Number(summary?.joined_observation_count) !== 2406 ||
    Number(summary?.missing_selected_observation_count) !== 0 ||
    Number(summary?.baseline_conflicting_context_count) !== 55 ||
    Number(summary?.joined_observations_in_baseline_conflicting_contexts) !== 227 ||
    Number(summary?.maximum_buffered_damage_rows_in_one_capture) !== 1878 ||
    Number(summary?.maximum_buffered_combat_rows_in_one_capture) !== 1938 ||
    !availableFields.includes("skill_effect_component_index") ||
    !availableFields.includes("capture_preceding_hp_loss_same_target") ||
    Number(lifecycle?.target_endpoint?.conflicting_repeated_context_count) !== 55 ||
    Number(lifecycle?.target_endpoint?.conflicting_repeated_observation_count) !== 227 ||
    Number(lifecycle?.source_and_target_endpoints?.conflicting_repeated_context_count) !== 55 ||
    proof?.policy?.lifecycle_affected_entity_is_allegiance_neutral !== true ||
    proof?.policy?.damage_target_is_allegiance_neutral !== true ||
    proof?.policy?.remote_player_cast_packets_required !== false ||
    proof?.policy?.formula_authority !== false ||
    proof?.policy?.runtime_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    proof?.conclusion?.same_capture_target_lifecycle_eliminates_all_conflicts !== false ||
    proof?.conclusion?.same_capture_source_and_target_lifecycle_eliminates_all_conflicts !== false ||
    proof?.conclusion?.group_relative_context_is_proven_formula_input !== false ||
    proof?.conclusion?.exact_damage_formula_proven !== false ||
    proof?.conclusion?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(proof?.observations) || proof.observations.length !== 2406) {
    throw new Error("Current-build group-relative topology diagnostic is unsafe or incomplete");
  }
  return {
    proof: receipt(attackGroupRelativeDiagnosticPath),
    selected_observations: 2406,
    baseline_conflicting_contexts: 55,
    baseline_conflicting_observations: 227,
    maximum_buffered_combat_rows: 1938,
    target_delta_index_semantics: "zero-based SyncNearDeltaInfo.deltas position",
    component_index_semantics:
      "zero-based AoiSyncDelta.skill_effects.damage position",
    same_capture_target_lifecycle_conflicting_contexts: 55,
    repeated_formula_controls_after_position_fields: 0,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackCurrentHpDiagnostic(proof, groupPath, build) {
  const groupReceipt = receipt(groupPath);
  const summary = proof?.summary;
  const available = proof?.diagnostics
    ?.original_conflict_reconstruction_available_subset_with_pre_hit_hp;
  const unresolved = new Map(
    (summary?.unresolved_reason_counts ?? []).map((row) => [row.value, Number(row.count)]),
  );
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "rlogs-bpsr-selected-hit-current-hp-diagnostic" ||
    String(proof?.game_build) !== String(build) ||
    Number(proof?.selection?.damage_attr_id) !== 2220352105 ||
    Number(proof?.selection?.action_id) !== 2203521 ||
    String(proof?.inputs?.group_relative_diagnostic?.sha256).toLowerCase() !==
      groupReceipt.sha256.toLowerCase() ||
    Number(proof?.inputs?.group_relative_diagnostic?.bytes) !== groupReceipt.bytes ||
    Number(proof?.inputs?.formula_cohort?.bytes) !== 1982846108 ||
    String(proof?.inputs?.formula_cohort?.sha256).toUpperCase() !==
      "C2F818F5FF2A4A3F9E41EA1EEC47F4556BE0582558AF903B078EC412959C7DB7" ||
    Number(proof?.inputs?.formula_cohort?.schema_version) !== 39 ||
    String(proof?.inputs?.formula_cohort?.game_build) !== String(build) ||
    Number(summary?.selected_observation_count) !== 2406 ||
    Number(summary?.requested_target_attribute_state_count) !== 2156 ||
    Number(summary?.extracted_target_attribute_state_count) !== 2156 ||
    Number(summary?.observations_with_wire_start_current_hp) !== 2383 ||
    Number(summary?.observations_with_wire_start_max_hp) !== 2383 ||
    Number(summary?.observations_with_reconstructed_pre_hit_hp) !== 2363 ||
    Number(summary?.conflicting_observations_with_reconstructed_pre_hit_hp) !== 220 ||
    unresolved.get("reconstructed-current-hp-negative") !== 20 ||
    unresolved.get("wire-start-current-hp-absent") !== 23 ||
    unresolved.get("wire-start-max-hp-absent") !== 23 ||
    Number(available?.context_count) !== 220 ||
    Number(available?.repeated_context_count) !== 0 ||
    Number(available?.conflicting_repeated_context_count) !== 0 ||
    proof?.policy?.formula_cohort_is_streamed_not_fully_deserialized !== true ||
    proof?.policy?.invalid_or_incomplete_hp_reconstruction_is_preserved_as_unresolved !== true ||
    proof?.policy?.reconstructed_hp_is_diagnostic_not_formula_authority !== true ||
    proof?.policy?.runtime_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    proof?.conclusion?.reconstructed_pre_hit_hp_available_rows_have_no_remaining_conflicts !== true ||
    proof?.conclusion?.reconstructed_pre_hit_hp_available_rows_retain_repeated_controls !== false ||
    proof?.conclusion?.reconstructed_pre_hit_hp_is_proven_server_formula_input !== false ||
    proof?.conclusion?.exact_hp_threshold_or_curve_proven !== false ||
    proof?.conclusion?.exact_damage_formula_proven !== false ||
    proof?.conclusion?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(proof?.observations) || proof.observations.length !== 2406) {
    throw new Error("Current-build CurrentHP diagnostic is unsafe or incomplete");
  }
  return {
    proof: receipt(attackCurrentHpDiagnosticPath),
    selected_observations: 2406,
    observations_with_reconstructed_pre_hit_hp: 2363,
    original_conflict_observations_with_reconstructed_pre_hit_hp: 220,
    repeated_equal_pre_hit_hp_controls: 0,
    impossible_negative_reconstructions: 20,
    observations_missing_current_and_max_hp: 23,
    current_hp_formula_authority: false,
    exact_hp_threshold_or_curve_proven: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackIntervalClosedHpDiagnostic(proof, groupPath, build) {
  const groupReceipt = receipt(groupPath);
  const summary = proof?.summary;
  const eligible = proof?.diagnostics?.interval_closed_exact_hp_subset;
  const eligibleConflicts = proof?.diagnostics
    ?.original_conflict_interval_closed_exact_hp_subset;
  const ledgerPath = proof?.inputs?.hp_ledger_proof?.path;
  if (typeof ledgerPath !== "string") {
    throw new Error("Current-build interval-closed HP diagnostic omits its ledger path");
  }
  const ledgerReceipt = receipt(ledgerPath);
  const ledger = readJson(ledgerPath);
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "rlogs-bpsr-selected-hit-interval-closed-hp-diagnostic" ||
    String(proof?.game_build) !== String(build) ||
    Number(proof?.selection?.damage_attr_id) !== 2220352105 ||
    Number(proof?.selection?.action_id) !== 2203521 ||
    String(proof?.inputs?.group_relative_diagnostic?.sha256).toLowerCase() !==
      groupReceipt.sha256.toLowerCase() ||
    Number(proof?.inputs?.group_relative_diagnostic?.bytes) !== groupReceipt.bytes ||
    String(proof?.inputs?.hp_ledger_proof?.sha256).toLowerCase() !==
      ledgerReceipt.sha256.toLowerCase() ||
    Number(proof?.inputs?.hp_ledger_proof?.bytes) !== ledgerReceipt.bytes ||
    Number(ledger?.schema_version) !== 2 ||
    ledger?.generated_by !== "rlogs-bpsr-hp-state-ledger-proof" ||
    Number(ledger?.aggregate?.eligible_intervals) !== 31292 ||
    Number(ledger?.aggregate?.eligible_exact) !== 83 ||
    Number(ledger?.aggregate?.eligible_mismatched) !== 31209 ||
    Number(ledger?.selected_action_hp_context?.requested_actions) !== 2406 ||
    Number(ledger?.selected_action_hp_context?.matched_actions) !== 2406 ||
    Number(summary?.selected_observations) !== 2406 ||
    Number(summary?.baseline_conflicting_observations) !== 227 ||
    Number(summary?.candidate_pre_hit_hp_observations) !== 2383 ||
    Number(summary?.interval_closed_exact_hp_observations) !== 3 ||
    Number(summary?.baseline_conflicting_observations_with_interval_closed_exact_hp) !== 0 ||
    Number(eligible?.context_count) !== 3 ||
    Number(eligible?.repeated_context_count) !== 0 ||
    Number(eligibleConflicts?.context_count) !== 0 ||
    proof?.policy?.complete_snapshot_interval_must_close_with_zero_residual !== true ||
    proof?.policy?.nonclosing_and_mismatched_intervals_are_preserved_as_unresolved !== true ||
    proof?.policy?.runtime_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    proof?.conclusion?.snapshot_transition_model_globally_validated !== false ||
    proof?.conclusion?.interval_closed_exact_hp_available_for_every_original_conflict !== false ||
    proof?.conclusion?.interval_closed_exact_hp_retains_repeated_conflict_controls !== false ||
    proof?.conclusion?.exact_hp_threshold_or_curve_proven !== false ||
    proof?.conclusion?.exact_damage_formula_proven !== false ||
    proof?.conclusion?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(proof?.observations) || proof.observations.length !== 2406) {
    throw new Error("Current-build interval-closed HP diagnostic is unsafe or incomplete");
  }
  return {
    proof: receipt(attackIntervalClosedHpDiagnosticPath),
    hp_ledger_proof: ledgerReceipt,
    selected_observations: 2406,
    candidate_pre_hit_hp_observations: 2383,
    interval_closed_exact_hp_observations: 3,
    original_conflict_observations_with_interval_closed_exact_hp: 0,
    repeated_equal_pre_hit_hp_controls: 0,
    globally_eligible_snapshot_intervals: 31292,
    globally_exact_snapshot_intervals: 83,
    snapshot_transition_model_formula_authority: false,
    exact_hp_threshold_or_curve_proven: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackTargetStaticContextDiagnostic(proof, groupPath, build) {
  const groupReceipt = receipt(groupPath);
  const inputNames = [
    "target_identity_proof",
    "monster_table",
    "entity_attribute_table",
    "bullet_table",
  ];
  for (const name of inputNames) {
    const declared = proof?.inputs?.[name];
    if (typeof declared?.path !== "string") {
      throw new Error(`Target static-context diagnostic omits ${name}`);
    }
    const actual = receipt(declared.path);
    if (Number(declared.bytes) !== actual.bytes ||
        String(declared.sha256).toLowerCase() !== actual.sha256.toLowerCase()) {
      throw new Error(`Target static-context diagnostic has stale ${name} receipt`);
    }
  }
  const summary = proof?.summary;
  const joined = proof?.diagnostics?.target_identity_level_and_static_signature;
  const conflicts = proof?.diagnostics
    ?.original_conflict_target_identity_level_and_static_signature;
  if (Number(proof?.schema_version) !== 1 ||
    proof?.generated_by !== "rlogs-bpsr-selected-hit-target-static-context-diagnostic" ||
    String(proof?.game_build) !== String(build) ||
    Number(proof?.selection?.damage_attr_id) !== 2220352105 ||
    Number(proof?.selection?.action_id) !== 2203521 ||
    String(proof?.inputs?.group_relative_diagnostic?.sha256).toLowerCase() !==
      groupReceipt.sha256.toLowerCase() ||
    Number(proof?.inputs?.group_relative_diagnostic?.bytes) !== groupReceipt.bytes ||
    Number(summary?.selected_observations) !== 2406 ||
    Number(summary?.exact_numeric_target_identities) !== 2406 ||
    Number(summary?.monster_target_observations) !== 2383 ||
    Number(summary?.projectile_target_observations) !== 23 ||
    Number(summary?.observations_with_level) !== 2383 ||
    Number(summary?.observations_with_exact_static_table_route) !== 2406 ||
    Number(summary?.observations_with_static_mitigation_scalar) !== 0 ||
    Number(summary?.distinct_static_targets) !== 18 ||
    Number(summary?.distinct_monster_targets) !== 16 ||
    Number(summary?.distinct_projectile_targets) !== 2 ||
    Number(summary?.baseline_conflicting_contexts) !== 55 ||
    Number(summary?.baseline_conflicting_observations) !== 227 ||
    Number(joined?.conflicting_repeated_context_count) !== 55 ||
    Number(joined?.conflicting_repeated_observation_count) !== 227 ||
    Number(conflicts?.conflicting_repeated_context_count) !== 55 ||
    Number(conflicts?.conflicting_repeated_observation_count) !== 227 ||
    proof?.policy?.target_allegiance_assumed !== false ||
    proof?.policy?.absent_static_mitigation_fields_are_not_zero !== true ||
    proof?.policy?.fight_value_coefficient_is_not_assumed_to_be_defense_or_mitigation !== true ||
    proof?.policy?.runtime_authority !== false ||
    proof?.policy?.provider_rdps_credit_allowed !== false ||
    proof?.conclusion?.every_selected_target_has_exact_numeric_identity !== true ||
    proof?.conclusion?.every_selected_target_has_exact_static_table_route !== true ||
    proof?.conclusion?.static_tables_supply_damage_mitigation_scalar !== false ||
    proof?.conclusion?.target_static_context_reduces_original_conflicts !== false ||
    proof?.conclusion?.exact_target_mitigation_formula_proven !== false ||
    proof?.conclusion?.exact_damage_formula_proven !== false ||
    proof?.conclusion?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(proof?.target_catalog) || proof.target_catalog.length !== 18 ||
    !Array.isArray(proof?.observations) || proof.observations.length !== 2406) {
    throw new Error("Current-build target static-context diagnostic is unsafe or incomplete");
  }
  return {
    proof: receipt(attackTargetStaticContextDiagnosticPath),
    target_identity_proof: receipt(proof.inputs.target_identity_proof.path),
    selected_observations: 2406,
    exact_numeric_target_identities: 2406,
    monster_target_observations: 2383,
    projectile_target_observations: 23,
    exact_static_table_routes: 2406,
    static_mitigation_scalars: 0,
    original_conflicting_contexts_after_static_join: 55,
    target_mitigation_formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackEffectiveStatWindowDiagnostic(proof, transitionPath, build) {
  if (!proof || !transitionPath) return null;
  const transitionReceipt = receipt(transitionPath);
  const declaredWindowSource = proof.effective_stat_window_selection?.source;
  if (typeof declaredWindowSource !== "string") {
    throw new Error("Effective-stat-window damage diagnostic omits its lifecycle source");
  }
  const declaredWindowReceipt = receipt(declaredWindowSource);
  const sessions = (proof.sessions ?? []).map((session) => ({
    session_id: String(session.session_id),
    damage_events_with_attack: Number(session.damage_events_with_attack),
    external_active_damage_events: Number(session.external_active_damage_events),
    inactive_damage_events: Number(session.inactive_damage_events),
    ambiguous_or_self_active_damage_events: Number(
      session.ambiguous_or_self_active_damage_events,
    ),
  }));
  const expectedSessions = [
    ["monitor-1787002076016.run-0004", 8460, 4423, 4037, 0],
    ["monitor-1787002076016.run-0010", 2486, 244, 1902, 340],
    ["monitor-1787003553387.run-0003", 11276, 3724, 7552, 0],
    ["monitor-1787003553387.run-0006", 16707, 4156, 11050, 1501],
  ].map(([session_id, damage, external, inactive, ambiguous]) => ({
    session_id,
    damage_events_with_attack: damage,
    external_active_damage_events: external,
    inactive_damage_events: inactive,
    ambiguous_or_self_active_damage_events: ambiguous,
  }));
  const overlap = Object.fromEntries(
    (proof.archetype_observed_counterfactuals?.overlap_diagnostics ?? []).map((row) => [
      String(row.stage),
      Number(row.keys_with_both_states),
    ]),
  );
  if (Number(proof.schema_version) !== 47 ||
    proof.generated_by !== "rlogs-bpsr-external-attack-damage-proof" ||
    Number(proof.selected_effect_id) !== 2110140 ||
    Number(proof.selected_source_config_id) !== 0 ||
    proof.selected_source_config_is_absent !== true ||
    Number(proof.final_attack_attribute_id) !== 11330 ||
    String(proof.damage_surface_identity?.game_build) !== String(build) ||
    proof.damage_surface_identity?.build_identity_verified !== true ||
    String(proof.effective_stat_window_selection?.game_build) !== String(build) ||
    Number(proof.effective_stat_window_selection?.exact_lifecycle_windows) !== 8 ||
    Number(proof.effective_stat_window_selection?.candidate_status_window_damage_actions) !==
      12557 ||
    Number(proof.effective_stat_window_selection?.effective_stat_window_damage_actions) !==
      12547 ||
    Number(proof.effective_stat_window_selection?.excluded_before_attribute_activation) !== 10 ||
    Number(proof.effective_stat_window_selection?.excluded_at_or_after_attribute_deactivation) !==
      0 ||
    proof.effective_stat_window_selection?.formula_authority !== false ||
    proof.effective_stat_window_selection?.provider_credit_allowed !== false ||
    declaredWindowReceipt.sha256.toLowerCase() !== transitionReceipt.sha256.toLowerCase() ||
    declaredWindowReceipt.bytes !== transitionReceipt.bytes ||
    JSON.stringify(sessions) !== JSON.stringify(expectedSessions) ||
    sessions.reduce((sum, row) => sum + row.external_active_damage_events, 0) !== 12547 ||
    sessions.reduce((sum, row) => sum + row.inactive_damage_events, 0) !== 24541 ||
    sessions.reduce((sum, row) => sum + row.ambiguous_or_self_active_damage_events, 0) !== 1841 ||
    Number(proof.archetype_observed_counterfactuals?.contexts_with_both_states) !== 0 ||
    Number(proof.archetype_observed_counterfactuals?.exact_contexts) !== 0 ||
    Number(overlap.stable_source_attributes) !== 0 ||
    proof.policy?.unresolved_evidence_is_hidden !== false) {
    throw new Error("Current-build effective-stat-window damage diagnostic is unsafe or incomplete");
  }
  return {
    proof: receipt(attackEffectiveStatWindowDiagnosticPath),
    exact_lifecycle_source: transitionReceipt,
    exact_lifecycle_windows: 8,
    effective_external_damage_actions: 12547,
    ordinary_inactive_damage_actions: 24541,
    status_presence_only_or_unproven_damage_actions: 1841,
    stable_source_control_overlaps: 0,
    exact_inactive_active_control_pairs: 0,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackTargetMitigationDiagnostic(identity, identityPath, proof, build) {
  if (!identity || !identityPath || !proof) return null;
  const identityReceipt = receipt(identityPath);
  const worklistPath = identity.selection_source;
  if (typeof worklistPath !== "string") {
    throw new Error("Target mitigation action identity omits its exact-key worklist");
  }
  const worklist = readJson(worklistPath);
  const worklistReceipt = receipt(worklistPath);
  const observations = identity.observations ?? [];
  const targetClassSpecialization117 = observations.filter((row) =>
    Number(row.class_id) === 11 && Number(row.specialization_id) === 117
  ).length;
  const targetClass11UnresolvedSpecialization = observations.filter((row) =>
    Number(row.class_id) === 11 && row.specialization_id == null
  ).length;
  const sourceEntityMismatches = observations.filter((row) =>
    (row.source_unresolved_reasons ?? []).includes("source-entity-mismatch")
  ).length;
  const sceneObservations = observations.filter((row) => row.scene_id != null).length;
  const summary = identity.summary;
  if (Number(worklist?.schema_version) !== 1 ||
    worklist?.generated_by !==
      "rlogs-bpsr-target-mitigation-transform-proof:target-identity-worklist" ||
    String(worklist?.game_build) !== String(build) ||
    Number(worklist?.summary?.requested_actions) !== 774 ||
    (worklist?.observations ?? []).length !== 774 ||
    worklist?.policy?.target_allegiance_assumed !== false ||
    worklist?.policy?.recipient_or_enemy_target_are_both_allowed !== true ||
    worklist?.policy?.remote_player_only_packets_are_zero_filled !== false ||
    worklist?.policy?.current_actor_snapshots_are_substituted !== false ||
    worklist?.policy?.provider_rdps_credit_allowed !== false ||
    Number(identity?.schema_version) !== 2 ||
    identity?.generated_by !== "rlogs-bpsr-selected-action-target-identity-proof" ||
    String(identity?.game_build) !== String(build) ||
    Number(summary?.requested_actions) !== 774 ||
    Number(summary?.matched_actions) !== 774 || Number(summary?.missing_actions) !== 0 ||
    Number(summary?.observations_with_active_actor_state) !== 774 ||
    Number(summary?.observations_with_identity_conflict) !== 0 ||
    Number(summary?.observations_with_active_source_actor_state) !== 773 ||
    Number(summary?.observations_with_exact_source_numeric_monster_id) !== 756 ||
    Number(summary?.observations_with_source_identity_conflict) !== 0 ||
    observations.length !== 774 ||
    observations.some((row) => row.actor_active !== true || row.actor_kind !== "player") ||
    targetClassSpecialization117 !== 754 ||
    targetClass11UnresolvedSpecialization !== 20 ||
    sourceEntityMismatches !== 0 || sceneObservations !== 764 ||
    identity?.policy?.target_endpoint_is_allegiance_neutral !== true ||
    identity?.policy?.recipient_or_enemy_target_are_both_allowed !== true ||
    identity?.policy?.absent_monster_or_character_identity_zero_filled !== false ||
    identity?.policy?.static_target_stats_substituted !== false ||
    identity?.policy?.runtime_authority !== false ||
    identity?.policy?.provider_rdps_credit_allowed !== false) {
    throw new Error("Current-build target mitigation action identity is unsafe or incomplete");
  }

  const enrichment = proof.target_identity_enrichment;
  const physical = proof.axes?.physical_defense;
  const magical = proof.axes?.magic_defense;
  const refined = proof.axes?.refined_defense;
  const expectedAxes = [
    [physical, 11350, 765, 747, [["runtime_simple_curve", 6500], ["transformed_curve", 22000]]],
    [magical, 11360, 765, 747, [["runtime_simple_curve", 6500], ["transformed_curve", 22000]]],
    [refined, 11420, 764, 756, [["runtime_simple_curve", 6500], ["transformed_curve", 9980]]],
  ];
  for (const [axis, attributeId, samples, contexts, models] of expectedAxes) {
    if (Number(axis?.current_attribute_id) !== attributeId ||
      Number(axis?.counters?.samples_with_axis) !== samples ||
      Number(axis?.counters?.samples_with_packet_observed_target_actor_identity) !== samples ||
      Number(axis?.counters?.samples_with_stable_target_actor_id) !== 0 ||
      Number(axis?.counters?.samples_with_cross_capture_actor_shape_context) !== contexts ||
      Number(axis?.counters?.groups_with_multiple_axis_states) !== 0 ||
      Number(axis?.counters?.distinct_axis_pairs) !== 0 ||
      Number(axis?.counters?.pairs_with_cross_capture_witness) !== 0) {
      throw new Error(`Target mitigation axis ${attributeId} changed or gained no exact proof`);
    }
    for (const [name, constant] of models) {
      const model = axis.models?.[name];
      if (Number(model?.constant) !== constant ||
        Number(model?.counters?.exact_pairs) !== 0 ||
        Number(model?.counters?.rejected_pairs) !== 0) {
        throw new Error(`Target mitigation model ${name} changed authority`);
      }
    }
  }
  if (Number(proof?.schema_version) !== 3 ||
    proof?.generated_by !==
      "rlogs-bpsr-target-mitigation-transform-proof:cross-capture-target-config-diagnostic" ||
    String(proof?.game_build) !== String(build) ||
    Number(proof?.input?.bytes) !== 1982846108 ||
    String(proof?.input?.sha256).toLowerCase() !==
      "sha256:c2f818f5ff2a4a3f9e41ea1eec47f4556be0582558af903b078ec412959c7db7" ||
    Number(proof?.processing?.sample_count) !== 735016 ||
    proof?.processing?.measured_peak_within_configured_limit !== true ||
    Number(enrichment?.declared_action_observations) !== 774 ||
    Number(enrichment?.declared_event_time_target_actor_observations) !== 774 ||
    Number(enrichment?.declared_event_time_source_actor_observations) !== 773 ||
    Number(enrichment?.declared_event_time_scene_observations) !== 764 ||
    Number(enrichment?.exact_formula_cohort_sample_joins) !== 774 ||
    Number(enrichment?.exact_formula_cohort_source_actor_joins) !== 773 ||
    Number(enrichment?.exact_formula_cohort_scene_joins) !== 764 ||
    Number(enrichment?.formula_cohort_identity_conflicts) !== 0 ||
    String(enrichment?.sha256).toLowerCase() !== `sha256:${identityReceipt.sha256}` ||
    Number(enrichment?.bytes) !== identityReceipt.bytes ||
    proof?.policy?.remote_player_only_packets_are_required !== false ||
    proof?.policy?.actor_identity_is_the_most_recent_packet_observed_actor_event_not_a_current_character_snapshot !== true ||
    proof?.policy?.cross_capture_witness_is_diagnostic_not_controlled_counterfactual_proof !== true ||
    proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
    proof?.authority?.exact_target_mitigation_formula_proven !== false ||
    proof?.authority?.exact_operation_order_and_integer_rounding_proven !== false ||
    proof?.authority?.packet_conservation_proven !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("Current-build target mitigation diagnostic is unsafe or incomplete");
  }
  return {
    action_identity_proof: identityReceipt,
    exact_key_worklist: worklistReceipt,
    diagnostic: receipt(attackTargetMitigationDiagnosticPath),
    exact_action_observations: 774,
    event_time_target_actor_observations: 774,
    event_time_source_actor_observations: 773,
    event_time_scene_observations: 764,
    target_actor_kind: "player",
    physical_defense_actor_scene_contexts: 747,
    magic_defense_actor_scene_contexts: 747,
    refined_defense_actor_scene_contexts: 756,
    controlled_axis_pairs: 0,
    candidate_constants: [6500, 9980, 22000],
    exact_target_mitigation_formula_proven: false,
    exact_operation_order_and_integer_rounding_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verifyAttackTargetMitigationClosure(
  offline,
  offlinePath,
  controlled,
  controlledPath,
  actionAudit,
  build,
) {
  if (!offline || !offlinePath || !controlled || !controlledPath || !actionAudit) {
    return null;
  }
  const offlineReceipt = receipt(offlinePath);
  const controlledReceipt = receipt(controlledPath);
  if (Number(offline?.schema_version) !== 4 ||
    offline?.generated_by !== "tools/target-mitigation-offline-exhaustion-proof.mjs" ||
    String(offline?.game_build) !== String(build) ||
    offline?.content_sha256 !== proofContentHash(offline) ||
    offline?.policy?.mitigation_action_targets_are_allegiance_neutral !== true ||
    offline?.policy?.current_actor_snapshots_are_never_substituted !== true ||
    offline?.policy?.exact_client_combat_damage_consumer_proven !== false ||
    offline?.policy?.server_combat_implementation_is_available_in_client_files !== false ||
    offline?.policy?.candidate_constants_are_combat_formula_authority !== false ||
    offline?.policy?.character_sheet_transform_is_combat_formula_authority !== false ||
    Number(offline?.summary?.final_validation_obligations) !== 2 ||
    Number(offline?.summary?.lua_files_scanned) !== 4821 ||
    Number(offline?.summary?.native_direct_callsites) !== 0 ||
    Number(offline?.summary?.exact_character_sheet_consumers) !== 2 ||
    Number(offline?.summary?.packet_capture_proofs) !== 24 ||
    Number(offline?.summary?.packet_source_rlogs) !== 26 ||
    Number(offline?.summary?.packet_damage_samples) !== 735016 ||
    Number(offline?.summary?.packet_audited_axis_samples) !== 2294 ||
    Number(offline?.summary?.controlled_counterfactual_pairs) !== 0 ||
    Number(offline?.summary?.neutral_mitigation_actions) !== 774 ||
    Number(offline?.summary?.neutral_player_targets) !== 774 ||
    Number(offline?.summary?.event_time_damage_actors) !== 773 ||
    Number(offline?.summary?.event_time_run_scenes) !== 764 ||
    Number(offline?.summary?.promoted_combat_formulas) !== 0 ||
    offline?.neutral_action_evidence?.provider_rdps_credit_allowed !== false ||
    String(offline?.inputs?.neutral_action_identity?.sha256).toLowerCase() !==
      actionAudit.action_identity_proof.sha256.toLowerCase() ||
    String(offline?.inputs?.neutral_mitigation_diagnostic?.sha256).toLowerCase() !==
      actionAudit.diagnostic.sha256.toLowerCase()) {
    throw new Error("Current-build target mitigation offline exhaustion proof is unsafe or incomplete");
  }

  const requiredVariants = controlled?.controlled_replay_contract?.required_variants ?? [];
  if (Number(controlled?.schema_version) !== 1 ||
    controlled?.generated_by !==
      "tools/bpsr-target-mitigation-controlled-replay-worklist.mjs" ||
    String(controlled?.game_build) !== String(build) ||
    controlled?.content_sha256 !== proofContentHash(controlled) ||
    controlled?.topology?.effect_edge !==
      "provider -> effect/status lifecycle -> recipient or enemy target" ||
    controlled?.topology?.damage_edge !==
      "damage actor -> numeric action -> recipient or enemy target" ||
    controlled?.topology?.effect_endpoint_allegiance_assumed !== false ||
    controlled?.topology?.damage_endpoint_allegiance_assumed !== false ||
    controlled?.policy?.target_allegiance_assumed !== false ||
    controlled?.policy?.current_actor_or_character_snapshot_substituted !== false ||
    controlled?.policy?.remote_player_packets_required !== false ||
    controlled?.policy?.unresolved_evidence_is_hidden !== false ||
    Number(controlled?.summary?.exact_actions_joined) !== 774 ||
    Number(controlled?.summary?.exact_player_targets) !== 774 ||
    Number(controlled?.summary?.exact_event_time_damage_actors) !== 773 ||
    Number(controlled?.summary?.exact_event_time_scenes) !== 764 ||
    Number(controlled?.summary?.physical_defense_values) !== 18 ||
    Number(controlled?.summary?.magic_defense_values) !== 12 ||
    Number(controlled?.summary?.refined_defense_values) !== 1 ||
    Number(controlled?.summary?.controlled_axis_pairs_available) !== 0 ||
    controlled?.summary?.client_combat_damage_consumer_proven !== false ||
    Number(controlled?.summary?.formulas_promoted) !== 0 ||
    controlled?.controlled_replay_contract?.topology !==
      "damage actor -> numeric action -> recipient or enemy target" ||
    !controlled?.controlled_replay_contract?.invariant_fields?.includes(
      "exact build and protocol pack",
    ) ||
    JSON.stringify(requiredVariants.map((row) => Number(row.axis_attribute_id))) !==
      JSON.stringify([11350, 11360, 11420, 13200]) ||
    controlled?.authority?.exact_target_mitigation_formula_proven !== false ||
    controlled?.authority?.exact_operation_order_and_integer_rounding_proven !== false ||
    controlled?.authority?.packet_conservation_proven !== false ||
    controlled?.authority?.provider_rdps_credit_allowed !== false ||
    String(controlled?.inputs?.offline_client_and_packet_exhaustion?.sha256).toLowerCase() !==
      offlineReceipt.sha256.toLowerCase() ||
    String(controlled?.inputs?.event_time_action_identity?.sha256).toLowerCase() !==
      actionAudit.action_identity_proof.sha256.toLowerCase() ||
    String(controlled?.inputs?.mitigation_action_worklist?.sha256).toLowerCase() !==
      actionAudit.exact_key_worklist.sha256.toLowerCase() ||
    !Array.isArray(controlled?.ranked_exact_action_contexts) ||
    controlled.ranked_exact_action_contexts.length === 0) {
    throw new Error("Current-build target mitigation controlled replay receipt is unsafe or incomplete");
  }
  return {
    offline_exhaustion_proof: offlineReceipt,
    controlled_replay_worklist: controlledReceipt,
    topology: controlled.topology,
    exact_build_client_combat_consumer_proven: false,
    exact_actions_ranked_for_controlled_replay: 774,
    observed_physical_defense_values: 18,
    observed_magic_defense_values: 12,
    observed_refined_defense_values: 1,
    isolated_controlled_axis_pairs: 0,
    exact_target_mitigation_formula_proven: false,
    exact_operation_order_and_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

async function compareTables(rows) {
  const result = {};
  for (const [id, fileName, priorSha256] of rows) {
    const filePath = path.join(repoRoot, "Excels", fileName);
    const current = await fingerprint(filePath);
    result[id] = {
      path: relative(filePath),
      bytes: current.bytes,
      current_sha256: current.sha256,
      prior_proven_sha256: priorSha256,
      exact_match: current.sha256 === priorSha256,
    };
  }
  return result;
}

function verifyNativeSignature({ filePath, rva, expected }) {
  const fd = fs.openSync(filePath, "r");
  try {
    const peOffsetBuffer = Buffer.alloc(4);
    fs.readSync(fd, peOffsetBuffer, 0, 4, 0x3c);
    const peOffset = peOffsetBuffer.readUInt32LE(0);
    const fileHeader = Buffer.alloc(24);
    fs.readSync(fd, fileHeader, 0, fileHeader.length, peOffset);
    if (fileHeader.toString("ascii", 0, 4) !== "PE\u0000\u0000") {
      throw new Error(`${filePath} is not a PE image`);
    }
    const sectionCount = fileHeader.readUInt16LE(6);
    const optionalHeaderSize = fileHeader.readUInt16LE(20);
    const sectionTableOffset = peOffset + 24 + optionalHeaderSize;
    let section = null;
    for (let index = 0; index < sectionCount; index += 1) {
      const row = Buffer.alloc(40);
      fs.readSync(fd, row, 0, row.length, sectionTableOffset + index * 40);
      const candidate = {
        name: row.toString("ascii", 0, 8).replace(/\u0000+$/, ""),
        virtualSize: row.readUInt32LE(8),
        virtualAddress: row.readUInt32LE(12),
        rawSize: row.readUInt32LE(16),
        rawPointer: row.readUInt32LE(20),
      };
      const span = Math.max(candidate.virtualSize, candidate.rawSize);
      if (rva >= candidate.virtualAddress && rva + expected.length <= candidate.virtualAddress + span) {
        section = candidate;
        break;
      }
    }
    if (!section) throw new Error(`RVA 0x${rva.toString(16)} is not mapped by a PE section`);
    const fileOffset = section.rawPointer + (rva - section.virtualAddress);
    const actual = Buffer.alloc(expected.length);
    fs.readSync(fd, actual, 0, actual.length, fileOffset);
    return {
      section: section.name,
      file_offset: `0x${fileOffset.toString(16)}`,
      expected_hex: expected.toString("hex"),
      actual_hex: actual.toString("hex"),
      exact_match: actual.equals(expected),
    };
  } finally {
    fs.closeSync(fd);
  }
}

function verifyDumpContracts(filePath, markers) {
  const dump = fs.readFileSync(filePath, "utf8");
  return {
    contracts: markers.map((marker) => ({ marker, present: dump.includes(marker) })),
  };
}

function runtimeComponent(proof, skillId, componentId) {
  const skill = proof.skills.find((entry) => Number(entry.imagine_skill_id) === Number(skillId));
  if (!skill) throw new Error(`Runtime Imagine skill ${skillId} is missing`);
  const component = (skill.components ?? []).find((entry) => entry.component_id === componentId);
  if (!component) throw new Error(`Runtime Imagine component ${skillId}:${componentId} is missing`);
  return component;
}

function compactRuntimeSummary(component) {
  return {
    status_rows: Number(component.summary?.status_rows ?? 0),
    self_status_rows: Number(component.summary?.self_status_rows ?? 0),
    external_player_status_rows: Number(component.summary?.external_player_status_rows ?? 0),
    equipped_provider_count: Number(component.summary?.unique_providers ?? 0),
    external_recipient_count: Number(
      component.summary?.unique_external_player_recipients ?? 0,
    ),
    applied_events: Number(component.summary?.lifecycle?.applied ?? 0),
    removed_events: Number(component.summary?.lifecycle?.removed ?? 0),
  };
}

function verifyFatalSpiralContracts({
  skillFightLevelTable,
  skillAoyiStarTable,
  skillTable,
  skillEffectTable,
  buffTable,
  familyProof,
  correlationProof,
  tempAttributeProof,
}) {
  const fightLevel = skillFightLevelTable["395701"];
  const skill = skillTable["3957"];
  const skillEffect = skillEffectTable["395701"];
  const buff = buffTable["2110125"];
  const tierRows = Object.values(skillAoyiStarTable)
    .filter((row) => Number(row.SkillId) === 3957)
    .sort((left, right) => Number(left.Level) - Number(right.Level));
  const baseAttrPer = numberParameter(fightLevel?.FloatParameter, "attrPer");
  const tierValues = tierRows.map((row) => {
    const tierAttrPer = numberParameter(row.FloatParameter, "attrPer");
    const totalBasisPoints = baseAttrPer + tierAttrPer;
    return {
      tier: Number(row.Level),
      tier_attr_per: tierAttrPer,
      total_basis_points: totalBasisPoints,
      percent: totalBasisPoints / 100,
    };
  });
  const family = (familyProof.families ?? []).find(
    (entry) => Number(entry.base_attribute_id) === 13100,
  );
  const pattern = (family?.update_patterns ?? []).find(
    (entry) =>
      JSON.stringify(entry.updated_offsets) === JSON.stringify([0, 1, 2]) &&
      Number(entry.count) >= 24,
  );
  const examples = pattern?.examples ?? [];
  const appliedExample = examples.find(
    (entry) =>
      entry.packet_values?.[0] === 1316 &&
      entry.packet_values?.[1] === 1316 &&
      entry.packet_values?.[2] === 1316,
  );
  const removedExample = examples.find(
    (entry) =>
      entry.packet_values?.[0] === 316 &&
      entry.packet_values?.[1] === 316 &&
      entry.packet_values?.[2] === 316 &&
      entry.deltas_since_last_seen?.[0] === -1000,
  );

  requireExact(fightLevel?.SkillId === 3957, "Fatal Spiral fight-level SkillId");
  requireExact(Number(fightLevel?.PVECoolTime) === 120, "Fatal Spiral cooldown");
  requireExact(baseAttrPer === 500, "Fatal Spiral base attrPer");
  requireExact(skill?.Name === "Arcane! Fatal Spiral", "Fatal Spiral localized identity");
  requireExact(skill?.EffectIDs?.includes(395701), "Fatal Spiral skill-effect route");
  requireExact(skillEffect?.SkillId === 3957, "Fatal Spiral effect owner");
  requireExact(
    (skillEffect?.SkillAttrDes ?? []).some(
      (entry) => entry?.[0] === "All-Element Bonus" && entry?.[1] === "",
    ),
    "Fatal Spiral all-element effect field",
  );
  requireExact(buff?.Name === "Highland Blood", "Fatal Spiral buff identity");
  requireExact(Number(buff?.DestroyParam?.[0]?.[1]) === 10, "Fatal Spiral duration");
  requireExact(
    JSON.stringify(tierValues.map((entry) => entry.tier)) === JSON.stringify([1, 2, 3, 4, 5]),
    "Fatal Spiral tier coverage",
  );
  requireExact(
    JSON.stringify(tierValues.map((entry) => entry.tier_attr_per)) ===
      JSON.stringify([100, 200, 300, 400, 500]),
    "Fatal Spiral tier additions",
  );
  requireExact(
    JSON.stringify(tierValues.map((entry) => entry.total_basis_points)) ===
      JSON.stringify([600, 700, 800, 900, 1000]),
    "Fatal Spiral combined tier values",
  );
  requireExact(
    Number(tempAttributeProof.totals?.current_build_unresolved_attribute_ids) === 0,
    "Fatal Spiral temporary-attribute inventory",
  );
  requireExact(Number(family?.attribute_events) === 24, "Fatal Spiral family event count");
  requireExact(Boolean(appliedExample), "Fatal Spiral +1000 packet oracle");
  requireExact(Boolean(removedExample), "Fatal Spiral -1000 packet oracle");
  requireExact(
    Number(correlationProof.effect_id) === 2110125 &&
      Number(correlationProof.status_event_count) === 120,
    "Fatal Spiral status/attribute correlation",
  );

  return {
    imagine_name: skill.Name,
    base_attr_per: baseAttrPer,
    tier_values: tierValues,
    duration_millis: Number(buff.DestroyParam[0][1]) * 1000,
    packet_attribute_oracle: {
      effect_id: 2110125,
      attribute_ids: [13100, 13101, 13102],
      tier: 5,
      baseline_value: 316,
      applied_value: 1316,
      applied_delta: 1000,
      removed_value: 316,
      removed_delta: -1000,
      correlated_status_events: Number(correlationProof.status_event_count),
      interpretation:
        "The packet-observed +1000/-1000 transition exactly matches the 500 base plus tier-5 500 fixed-point total.",
    },
  };
}

function verifySuperconductorContracts({
  skillFightLevelTable,
  skillAoyiTable,
  skillAoyiStarTable,
  skillTable,
  skillEffectTable,
  buffTable,
}) {
  const fightLevel = skillFightLevelTable["397101"];
  const imagine = skillAoyiTable["3971"];
  const skill = skillTable["3971"];
  const skillEffect = skillEffectTable["397101"];
  const buff = buffTable["2110140"];
  const baseParameterPair = [
    numberParameter(fightLevel?.FloatParameter, "attrA"),
    numberParameter(fightLevel?.FloatParameter, "attrB"),
  ];
  const tierRows = Object.values(skillAoyiStarTable)
    .filter((row) => Number(row.SkillId) === 3971)
    .sort((left, right) => Number(left.Level) - Number(right.Level));
  const tierParameterPairs = Object.fromEntries(
    tierRows.map((row) => [String(Number(row.Level)), (row.BuffPar?.[0] ?? []).map(Number)]),
  );
  const starIncrementParameterPairs = Object.fromEntries(
    tierRows.map((row) => [
      String(Number(row.Level)),
      [
        numberParameter(row.FloatParameter, "attrA"),
        numberParameter(row.FloatParameter, "attrB"),
      ],
    ]),
  );
  const loadoutTierParameterPairs = {
    0: baseParameterPair,
    ...tierParameterPairs,
  };
  const expectedTierPairs = {
    1: [780, 1040],
    2: [960, 1280],
    3: [1140, 1520],
    4: [1320, 1760],
    5: [1500, 2000],
  };
  const expectedStarIncrements = {
    1: [150, 200],
    2: [300, 400],
    3: [450, 600],
    4: [600, 800],
    5: [750, 1000],
  };

  requireExact(Number(fightLevel?.SkillId) === 3971, "Superconductor fight-level SkillId");
  requireExact(Number(fightLevel?.SkillEffectId) === 397101, "Superconductor effect route");
  requireExact(Number(fightLevel?.PVECoolTime) === 120, "Superconductor cooldown");
  requireExact(
    JSON.stringify(baseParameterPair) === JSON.stringify([750, 1000]),
    "Superconductor base parameter pair",
  );
  requireExact(Number(imagine?.AoyiItemId) === 3000123, "Superconductor item identity");
  requireExact(Number(imagine?.ResonanceMaxLv) === 5, "Superconductor maximum tier");
  requireExact(
    (imagine?.TransformationType ?? []).some(
      (entry) => Number(entry?.[0]) === 3 && Number(entry?.[1]) === 3200038,
    ),
    "Superconductor numeric description route",
  );
  requireExact(
    JSON.stringify(tierRows.map((row) => Number(row.Level))) ===
      JSON.stringify([1, 2, 3, 4, 5]),
    "Superconductor tier coverage",
  );
  requireExact(
    JSON.stringify(tierParameterPairs) === JSON.stringify(expectedTierPairs),
    "Superconductor combined tier parameter pairs",
  );
  requireExact(
    JSON.stringify(starIncrementParameterPairs) === JSON.stringify(expectedStarIncrements),
    "Superconductor star increment parameter pairs",
  );
  requireExact(
    tierRows.every((row) =>
      (row.TransformationType ?? []).some(
        (entry) => Number(entry?.[0]) === 7 && Number(entry?.[1]) === 3971 &&
          Number(entry?.[2]) === Number(row.Level),
      )),
    "Superconductor numeric tier transformation routes",
  );
  requireExact(skill?.EffectIDs?.includes(397101), "Superconductor skill-effect identity");
  requireExact(Number(skillEffect?.SkillId) === 3971, "Superconductor skill-effect owner");
  requireExact(
    (skillEffect?.SkillAttrDes ?? []).some((entry) => entry?.[0] === "Main Attribute Enhanced") &&
      (skillEffect?.SkillAttrDes ?? []).some((entry) => entry?.[0] === "Healing Received up"),
    "Superconductor localized component evidence",
  );
  requireExact(Number(buff?.DestroyParam?.[0]?.[1]) === 15, "Superconductor duration");

  return {
    imagine_name_evidence: String(skill?.Name ?? ""),
    base_parameter_pair: baseParameterPair,
    tier_parameter_pairs: tierParameterPairs,
    loadout_tier_parameter_pairs: loadoutTierParameterPairs,
    star_increment_parameter_pairs: starIncrementParameterPairs,
  };
}

function numberParameter(parameters, key) {
  const pair = (parameters ?? []).find((entry) => entry?.[0] === key);
  const value = Number(pair?.[1]);
  if (!Number.isFinite(value)) throw new Error(`Missing numeric parameter ${key}`);
  return value;
}

function requireExact(condition, label) {
  if (!condition) throw new Error(`${label} does not match the proven current-build contract`);
}

function assertBuild(value, expected, label) {
  if (String(value.game_build) !== String(expected)) {
    throw new Error(`${label} build ${value.game_build} does not match ${expected}`);
  }
}

async function fingerprint(filePath) {
  const stat = fs.statSync(filePath);
  const hash = crypto.createHash("sha256");
  await new Promise((resolve, reject) => {
    const stream = fs.createReadStream(filePath);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("error", reject);
    stream.on("end", resolve);
  });
  return { path: relative(filePath), bytes: stat.size, sha256: hash.digest("hex") };
}

function receipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: relative(filePath),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function proofContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function relative(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}
