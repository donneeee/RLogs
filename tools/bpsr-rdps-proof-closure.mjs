#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
const PROOF_CLOSURE_SCHEMA_VERSION = 79;

const FIGHT_SOURCE_ENUM_24687926 = new Map([
  [0, ["skill", "EFightSourceSkill"]],
  [1, ["buff", "EFightSourceBuff"]],
  [2, ["bullet", "EFightSourceBullet"]],
  [4, ["task", "EFightSourceTask"]],
  [6, ["talent", "EFightSourceTalent"]],
  [7, ["season-medal", "EFightSourceSeasonMedal"]],
  [8, ["union-effect", "EFightSourceUnionEffect"]],
  [9, ["mod", "EFightSourceMod"]],
  [10, ["equip", "EFightSourceEquip"]],
  [11, ["equip-slot-refine", "EFightSourceEquipSlotRefine"]],
  [12, ["vehicle", "EFightSourceVehicle"]],
  [13, ["season-talent", "EFightSourceSeasonTalent"]],
  [14, ["fantasy-atlas", "EFightSourceFantasyAtlas"]],
  [1000, ["scene-begin", "EFightSourceSceneBegin"]],
  [1001, ["scene", "EFightSourceScene"]],
  [1002, ["affix", "EFightSourceAffix"]],
  [10000, ["other", "EFightSourceOther"]],
]);

const EXTERNAL_TRANSFER_GATE_KINDS = new Set([
  "external-recipient-counterfactual",
  "external-target-state-counterfactual",
]);
const NONTRANSFER_GATE_KINDS = new Set([
  "self-only-nontransfer",
  "mixed-known-nontransfer",
  "source-owned-output-nontransfer",
]);
const UNRESOLVED_TRANSFER_GATE_KINDS = new Set([
  "unresolved-provider-recipient-hold",
  "unresolved-target-filtered-hold",
  "owner-local-formula-context-scope-hold",
  "mixed-source-output-and-open-owner-context-hold",
]);
const OFFLINE_FORMULA_PROOF_STATE = "exact-current-build-offline-formula-proven";
const CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE = "exact-current-build-canonical-runtime-input-route-proven";
const ALLOWED_SHARED_PROOF_STATES = new Set([
  OFFLINE_FORMULA_PROOF_STATE,
  CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE,
]);
const EXTERNAL_RUNTIME_EFFECT_STATUSES = new Set([
  "runtime-attribution-promoted-exact-subset",
  "runtime-model-ready-awaiting-strict-conservation",
  "runtime-external-open",
]);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "inspect") inspect(path.resolve(required(options, "input")), options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    manifest: path.resolve(required(parsed, "manifest")),
    aggregate: path.resolve(required(parsed, "aggregate")),
    staticFormulaEvidence: path.resolve(required(parsed, "static-formula-evidence")),
    workbench: path.resolve(required(parsed, "workbench")),
    carryForward: path.resolve(required(parsed, "carry-forward")),
    runtimeAttributionEvidence: parsed["runtime-attribution-evidence"]
      ? path.resolve(parsed["runtime-attribution-evidence"])
      : null,
    counterfactualFrontier: parsed["counterfactual-frontier"]
      ? path.resolve(parsed["counterfactual-frontier"])
      : null,
    counterfactualRollups: (parsed["counterfactual-rollup"] ?? []).map((input) =>
      path.resolve(input)
    ),
    providerOwnershipProofs: (parsed["provider-ownership-proof"] ?? []).map((input) =>
      path.resolve(input)
    ),
    statusEventSeasonContextProofs:
      (parsed["status-event-season-context-proof"] ?? []).map((input) => path.resolve(input)),
    seasonStateMutationProof: parsed["season-state-mutation-proof"]
      ? path.resolve(parsed["season-state-mutation-proof"])
      : null,
    partyHasteStackingFrontier: parsed["party-haste-stacking-frontier"]
      ? path.resolve(parsed["party-haste-stacking-frontier"])
      : null,
    actionSpeedFormulaProof: parsed["action-speed-formula-proof"]
      ? path.resolve(parsed["action-speed-formula-proof"])
      : null,
    partyHasteCapacityProof: parsed["party-haste-capacity-proof"]
      ? path.resolve(parsed["party-haste-capacity-proof"])
      : null,
    actionTimingAncestryProof: parsed["action-timing-ancestry-proof"]
      ? path.resolve(parsed["action-timing-ancestry-proof"])
      : null,
    imagineFormulaProof: parsed["imagine-formula-proof"]
      ? path.resolve(parsed["imagine-formula-proof"])
      : null,
    imagineStatusAttributeTierProof: parsed["imagine-status-attribute-tier-proof"]
      ? path.resolve(parsed["imagine-status-attribute-tier-proof"])
      : null,
    imagineTierWindowCounterfactualInputs:
      parsed["imagine-tier-window-counterfactual-inputs"]
        ? path.resolve(parsed["imagine-tier-window-counterfactual-inputs"])
        : null,
    fatalSpiralDamageStageFrontier: parsed["fatal-spiral-damage-stage-frontier"]
      ? path.resolve(parsed["fatal-spiral-damage-stage-frontier"])
      : null,
    targetVulnerabilityFormulaProof: parsed["target-vulnerability-formula-proof"]
      ? path.resolve(parsed["target-vulnerability-formula-proof"])
      : null,
    integerTransformConstraints: (parsed["integer-transform-constraints"] ?? []).map((input) =>
      path.resolve(input)
    ),
    componentScalarProofs: (parsed["component-scalar-proof"] ?? []).map((input) =>
      path.resolve(input)
    ),
    supportEffectProofs: (parsed["support-effect-proof"] ?? []).map((input) =>
      path.resolve(input)
    ),
    lifeWaveTriggerProof: parsed["life-wave-trigger-proof"]
      ? path.resolve(parsed["life-wave-trigger-proof"])
      : null,
    lifeWaveRemoteInferenceProof: parsed["life-wave-remote-inference-proof"]
      ? path.resolve(parsed["life-wave-remote-inference-proof"])
      : null,
    criticalDamageFactorInterpretationProof: parsed["critical-damage-factor-interpretation-proof"]
      ? path.resolve(parsed["critical-damage-factor-interpretation-proof"])
      : null,
    runtimeEffectComponentRoutingProof: path.resolve(required(parsed, "runtime-effect-component-routing-proof")),
    partySkillStaticClosure: path.resolve(required(parsed, "party-skill-static-closure")),
    partyEffectWindowAudit: path.resolve(required(parsed, "party-effect-window-audit")),
    protocolStatus: parsed["protocol-status"]
      ? path.resolve(parsed["protocol-status"])
      : null,
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  const started = performance.now();
  for (const [label, file] of Object.entries({
    manifest: context.manifest,
    "static formula evidence": context.staticFormulaEvidence,
    "formula model workbench": context.workbench,
    "formula proof carry-forward": context.carryForward,
    "runtime effect component routing proof": context.runtimeEffectComponentRoutingProof,
    "party-skill static closure": context.partySkillStaticClosure,
    "party-effect window audit": context.partyEffectWindowAudit,
  })) requireFile(file, label);

  const manifest = readJson(context.manifest, "proof correlation manifest");
  const aggregatePresent = existsSync(context.aggregate);
  const aggregateDocument = aggregatePresent
    ? readJson(context.aggregate, "proof correlation aggregate")
    : null;
  const aggregate = aggregatePresent
    ? aggregateDocument.aggregate ?? aggregateDocument
    : emptyAggregate(context.build, manifest);
  const staticEvidence = readJson(context.staticFormulaEvidence, "static formula evidence");
  const workbench = readJson(context.workbench, "formula model workbench");
  const carryForward = readJson(context.carryForward, "formula proof carry-forward");
  const runtimeAttributionEvidence = context.runtimeAttributionEvidence
    ? readJson(context.runtimeAttributionEvidence, "runtime attribution evidence")
    : null;
  const counterfactualFrontier = context.counterfactualFrontier
    ? readJson(context.counterfactualFrontier, "status-effect counterfactual frontier")
    : null;
  const counterfactualRollupPaths = context.counterfactualRollups ??
    (context.counterfactualRollup ? [context.counterfactualRollup] : []);
  const counterfactualRollups = counterfactualRollupPaths.map((input) =>
    readJson(input, "status-effect counterfactual rollup")
  );
  const providerOwnershipProofPaths = context.providerOwnershipProofs ?? [];
  const providerOwnershipProofs = providerOwnershipProofPaths.map((input) =>
    readJson(input, "status-effect provider ownership proof")
  );
  const statusEventSeasonContextProofPaths = context.statusEventSeasonContextProofs ?? [];
  const statusEventSeasonContextProofs = statusEventSeasonContextProofPaths.map((input) =>
    readJson(input, "status-event season-context proof")
  );
  const seasonStateMutationProof = context.seasonStateMutationProof
    ? readJson(context.seasonStateMutationProof, "season-state mutation proof")
    : null;
  const partyHasteStackingFrontier = context.partyHasteStackingFrontier
    ? readJson(context.partyHasteStackingFrontier, "party-Haste stacking frontier")
    : null;
  const actionSpeedFormulaProof = context.actionSpeedFormulaProof
    ? readJson(context.actionSpeedFormulaProof, "native action-speed formula proof")
    : null;
  const partyHasteCapacityProof = context.partyHasteCapacityProof
    ? readJson(context.partyHasteCapacityProof, "party-Haste conditional-capacity proof")
    : null;
  const actionTimingAncestryProof = context.actionTimingAncestryProof
    ? readJson(context.actionTimingAncestryProof, "action timing ancestry proof")
    : null;
  const imagineFormulaProof = context.imagineFormulaProof
    ? readJson(context.imagineFormulaProof, "Imagine formula proof")
    : null;
  const imagineStatusAttributeTierProof = context.imagineStatusAttributeTierProof
    ? readJson(
      context.imagineStatusAttributeTierProof,
      "Imagine status-attribute tier proof",
    )
    : null;
  const imagineTierWindowCounterfactualInputs =
    context.imagineTierWindowCounterfactualInputs
      ? readJson(
        context.imagineTierWindowCounterfactualInputs,
        "Imagine tier-window counterfactual inputs",
      )
      : null;
  const fatalSpiralDamageStageFrontier = context.fatalSpiralDamageStageFrontier
    ? readJson(
      context.fatalSpiralDamageStageFrontier,
      "Fatal Spiral damage-stage frontier",
    )
    : null;
  const targetVulnerabilityFormulaProof = context.targetVulnerabilityFormulaProof
    ? readJson(context.targetVulnerabilityFormulaProof, "target-vulnerability formula proof")
    : null;
  const integerTransformConstraintPaths = context.integerTransformConstraints ?? [];
  const integerTransformConstraintDocuments = integerTransformConstraintPaths.map((input) =>
    readJson(input, "status-effect integer transform constraints")
  );
  const componentScalarProofPaths = context.componentScalarProofs ?? [];
  const componentScalarProofDocuments = componentScalarProofPaths.map((input) =>
    readJson(input, "component static scalar proof")
  );
  const supportEffectProofPaths = context.supportEffectProofs ?? [];
  const supportEffectProofDocuments = supportEffectProofPaths.map((input) =>
    readJson(input, "support-effect proof")
  );
  const lifeWaveTriggerProof = context.lifeWaveTriggerProof
    ? readJson(context.lifeWaveTriggerProof, "Life Wave trigger proof")
    : null;
  const lifeWaveRemoteInferenceProof = context.lifeWaveRemoteInferenceProof
    ? readJson(context.lifeWaveRemoteInferenceProof, "Life Wave remote inference proof")
    : null;
  const criticalDamageFactorInterpretationProof = context.criticalDamageFactorInterpretationProof
    ? readJson(
      context.criticalDamageFactorInterpretationProof,
      "critical-damage factor interpretation proof",
    )
    : null;
  const runtimeEffectComponentRoutingProof = readJson(
    context.runtimeEffectComponentRoutingProof,
    "runtime effect component routing proof",
  );
  const partySkillStaticClosure = readJson(
    context.partySkillStaticClosure,
    "party-skill static closure",
  );
  const partyEffectWindowAudit = readJson(
    context.partyEffectWindowAudit,
    "party-effect window audit",
  );
  const protocolStatus = context.protocolStatus
    ? readJson(context.protocolStatus, "protocol-pack status")
    : null;
  requireBuild(manifest.game_build, context.build, "proof correlation manifest");
  requireBuild(aggregate.manifest_game_build, context.build, "proof correlation aggregate");
  requireBuild(staticEvidence.game_build, context.build, "static formula evidence");
  requireBuild(workbench.game_build, context.build, "formula model workbench");
  requireBuild(carryForward.build_id, context.build, "formula proof carry-forward");
  requireBuild(
    runtimeEffectComponentRoutingProof.game_build,
    context.build,
    "runtime effect component routing proof",
  );
  requireBuild(
    partySkillStaticClosure.game_build,
    context.build,
    "party-skill static closure",
  );
  requireBuild(
    partyEffectWindowAudit.game_build,
    context.build,
    "party-effect window audit",
  );
  if (protocolStatus) requireBuild(protocolStatus.game_build, context.build, "protocol-pack status");
  if (counterfactualFrontier) {
    requireBuild(counterfactualFrontier.game_build, context.build, "status-effect counterfactual frontier");
  }
  for (const counterfactualRollup of counterfactualRollups) {
    requireBuild(counterfactualRollup.game_build, context.build, "status-effect counterfactual rollup");
  }
  const providerOwnershipIndex = buildProviderOwnershipIndex(
    providerOwnershipProofs,
    providerOwnershipProofPaths,
    context.build,
  );
  const statusEventSeasonContextIndex = buildStatusEventSeasonContextIndex(
    statusEventSeasonContextProofs,
    statusEventSeasonContextProofPaths,
    context.build,
  );
  const seasonStateMutationReceipt = validateSeasonStateMutationProof(
    seasonStateMutationProof,
    context.seasonStateMutationProof,
    context.build,
  );
  const partyHasteStackingReceipt = validatePartyHasteStackingFrontier(
    partyHasteStackingFrontier,
    context.partyHasteStackingFrontier,
    context.partyEffectWindowAudit,
    context.build,
  );
  const partyHasteActionReceipt = validatePartyHasteActionFrontier(
    actionSpeedFormulaProof,
    context.actionSpeedFormulaProof,
    partyHasteCapacityProof,
    context.partyHasteCapacityProof,
    actionTimingAncestryProof,
    context.actionTimingAncestryProof,
    context.build,
  );
  const imagineFormulaReceiptIndex = validateImagineFormulaProof(
    imagineFormulaProof,
    context.imagineFormulaProof,
    context.build,
  );
  const imagineStatusAttributeTierReceipt = validateImagineStatusAttributeTierProof(
    imagineStatusAttributeTierProof,
    context.imagineStatusAttributeTierProof,
    context.imagineFormulaProof,
    context.build,
  );
  if (imagineStatusAttributeTierReceipt) {
    const superconductor = imagineFormulaReceiptIndex.get(2110140);
    if (!superconductor) {
      throw new Error("Imagine status-attribute tier proof requires the matching formula proof");
    }
    superconductor.status_attribute_tier_evidence = imagineStatusAttributeTierReceipt;
  }
  const imagineTierWindowCounterfactualReceipt =
    validateImagineTierWindowCounterfactualInputs(
      imagineTierWindowCounterfactualInputs,
      context.imagineTierWindowCounterfactualInputs,
      context.imagineStatusAttributeTierProof,
      context.imagineFormulaProof,
      context.build,
    );
  if (imagineTierWindowCounterfactualReceipt) {
    const superconductor = imagineFormulaReceiptIndex.get(2110140);
    if (!superconductor?.status_attribute_tier_evidence) {
      throw new Error(
        "Imagine tier-window counterfactual inputs require matching formula and tier proofs",
      );
    }
    superconductor.tier_window_counterfactual_inputs =
      imagineTierWindowCounterfactualReceipt;
  }
  const fatalSpiralDamageStageReceipt = validateFatalSpiralDamageStageFrontier(
    fatalSpiralDamageStageFrontier,
    context.fatalSpiralDamageStageFrontier,
    context.build,
  );
  if (fatalSpiralDamageStageReceipt) {
    const fatalSpiral = imagineFormulaReceiptIndex.get(2110125);
    if (!fatalSpiral) {
      throw new Error("Fatal Spiral damage-stage frontier requires the matching Imagine formula proof");
    }
    fatalSpiral.damage_stage_frontier = fatalSpiralDamageStageReceipt;
    fatalSpiral.provider_tier_snapshot_complete = true;
    fatalSpiral.affected_hit_rows_selected = true;
  }
  const targetVulnerabilityFormulaReceipt = validateTargetVulnerabilityFormulaProof(
    targetVulnerabilityFormulaProof,
    context.targetVulnerabilityFormulaProof,
    context.build,
  );
  const integerTransformConstraintIndex = buildIntegerTransformConstraintIndex(
    integerTransformConstraintDocuments,
    integerTransformConstraintPaths,
    context.build,
  );
  const componentScalarProofIndex = buildComponentScalarProofIndex(
    componentScalarProofDocuments,
    componentScalarProofPaths,
    context.build,
  );
  const componentScalarFrontierResults = [...componentScalarProofIndex.values()]
    .sort((left, right) => compareIdentifiers(left.effect_id, right.effect_id));
  const supportEffectFrontierResults = buildSupportEffectFrontierResults(
    supportEffectProofDocuments,
    supportEffectProofPaths,
    context.build,
  );
  const lifeWaveTriggerFrontier = validateLifeWaveTriggerProof(
    lifeWaveTriggerProof,
    context.lifeWaveTriggerProof,
    context.build,
  );
  const lifeWaveRemoteInferenceFrontier = validateLifeWaveRemoteInferenceProof(
    lifeWaveRemoteInferenceProof,
    context.lifeWaveRemoteInferenceProof,
    context.lifeWaveTriggerProof,
    context.build,
  );
  const partySkillFrontier = buildPartySkillFrontier(
    partySkillStaticClosure,
    context.partySkillStaticClosure,
    context.build,
  );
  const partyEffectWindowFrontier = buildPartyEffectWindowFrontier(
    partyEffectWindowAudit,
    context.partyEffectWindowAudit,
    partySkillFrontier,
    supportEffectFrontierResults,
    providerOwnershipIndex,
    statusEventSeasonContextIndex,
    seasonStateMutationReceipt,
    partyHasteStackingReceipt,
    partyHasteActionReceipt,
    imagineFormulaReceiptIndex,
    context.build,
  );
  attachTargetVulnerabilityFormulaReceipt(
    partyEffectWindowFrontier,
    targetVulnerabilityFormulaReceipt,
  );
  attachCriticalDamageFactorInterpretationProof(
    supportEffectFrontierResults,
    criticalDamageFactorInterpretationProof,
    context.criticalDamageFactorInterpretationProof,
    context.build,
  );

  const aggregateIndex = buildAggregateObservationIndex(aggregate.obligations ?? []);
  const terminalEffectIndex = buildTerminalEffectObservationIndex(
    aggregate.dreamscope_terminal_effects ?? [],
  );
  const usedAggregateObligationIds = new Set();
  const staticBySource = uniqueIndex(staticEvidence.sources ?? [], "source_rule_id", "static formula source");
  const workbenchModelsBySource = indexWorkbenchModels(workbench.model_groups ?? []);
  const historicalProofsByEffect = indexHistoricalProofs(carryForward.proofs ?? []);
  const obligationResults = [];

  for (const obligation of manifest.obligations ?? []) {
    const correlationMatch = resolveAggregateObservation(
      obligation,
      aggregateIndex,
      usedAggregateObligationIds,
    );
    const observed = enrichObservationWithExactTerminalEffects(
      correlationMatch.observed,
      obligation,
      terminalEffectIndex,
    );
    const sourceRuleIds = uniqueSorted(obligation.selectors?.source_rule_ids ?? []);
    const result = evaluateObligation({
      build: context.build,
      manifestObligation: obligation,
      observed,
      aggregate,
      sourceRuleIds,
      staticBySource,
      workbenchModelsBySource,
      historicalProofsByEffect,
      componentScalarProofIndex,
    });
    result.correlation_match = {
      kind: correlationMatch.kind,
      manifest_obligation_id: String(obligation.obligation_id),
      aggregate_obligation_id: String(observed.obligation_id),
      runtime_selector_contract_sha256: correlationMatch.runtime_selector_contract_sha256,
    };
    obligationResults.push(result);
  }
  obligationResults.sort((left, right) => compareText(left.obligation_id, right.obligation_id));

  const sourceResults = buildSourceResults(obligationResults, staticBySource, workbenchModelsBySource);
  const modelResults = buildModelResults(workbench, obligationResults);
  const runtimeEffectResults = buildRuntimeEffectResults(
    aggregate,
    runtimeAttributionEvidence,
    runtimeEffectComponentRoutingProof,
    context.build,
  );
  const counterfactualFrontierResults = uniqueCounterfactualResultLoci([
    ...buildCounterfactualFrontierResults(counterfactualFrontier, context.build),
    ...counterfactualRollups.flatMap((rollup) =>
      buildCounterfactualRollupResults(
        rollup,
        context.build,
        providerOwnershipIndex.get(Number(rollup.effect_id)) ?? null,
        integerTransformConstraintIndex,
        componentScalarProofIndex,
      )
    ),
  ]);
  const protocolPromotionGate = buildProtocolPromotionGate(protocolStatus);
  for (const effect of runtimeEffectResults) {
    effect.production_runtime_credit_allowed = protocolPromotionGate.runtime_promotion_allowed &&
      effect.status === "runtime-attribution-promoted-exact-subset";
  }
  const summary = summarize({ obligationResults, sourceResults, modelResults, runtimeEffectResults, counterfactualFrontierResults, manifest, aggregate, aggregatePresent, staticEvidence, workbench });
  summary.runtime_promotable_obligations = protocolPromotionGate.runtime_promotion_allowed
    ? summary.strictly_promotable_obligations
    : 0;
  summary.packet_observed_runtime_production_credit_allowed_effects = runtimeEffectResults.filter(
    (effect) => effect.production_runtime_credit_allowed,
  ).length;
  summary.support_effect_frontier_results = supportEffectFrontierResults.length;
  summary.component_scalar_frontier_results = componentScalarFrontierResults.length;
  summary.component_scalar_open_damage_projections = componentScalarFrontierResults.filter(
    (effect) => !effect.exact_damage_projection_proven,
  ).length;
  summary.component_scalar_provider_credit_allowed = componentScalarFrontierResults.filter(
    (effect) => effect.provider_rdps_credit_allowed,
  ).length;
  summary.support_effect_exact_stat_transforms = supportEffectFrontierResults.filter(
    (effect) => effect.exact_stat_transform_proven,
  ).length;
  summary.support_effect_open_opportunity_formulas = supportEffectFrontierResults.filter(
    (effect) => !effect.opportunity_counterfactual_proven,
  ).length;
  summary.support_effect_provider_credit_allowed = supportEffectFrontierResults.filter(
    (effect) => effect.provider_rdps_credit_allowed,
  ).length;
  summary.support_effect_frontier_complete = supportEffectFrontierResults.every(
    (effect) => effect.provider_rdps_credit_allowed,
  );
  summary.life_wave_trigger_proof_receipts = lifeWaveTriggerFrontier ? 1 : 0;
  summary.life_wave_refresh_activations =
    lifeWaveTriggerFrontier?.activation_count ?? 0;
  summary.life_wave_reprocs_before_expiry =
    lifeWaveTriggerFrontier?.same_instance_reproc_before_expiry_count ?? 0;
  summary.life_wave_unique_external_heal_candidate_activations =
    lifeWaveTriggerFrontier?.unique_external_heal_candidate_activations ?? 0;
  summary.life_wave_unique_self_heal_candidate_activations =
    lifeWaveTriggerFrontier?.unique_self_heal_candidate_activations ?? 0;
  summary.life_wave_ambiguous_heal_candidate_activations =
    lifeWaveTriggerFrontier?.ambiguous_heal_candidate_activations ?? 0;
  summary.life_wave_no_heal_candidate_activations =
    lifeWaveTriggerFrontier?.no_heal_candidate_activations ?? 0;
  summary.life_wave_remote_inference_proof_receipts =
    lifeWaveRemoteInferenceFrontier ? 1 : 0;
  summary.life_wave_remote_inference_damage_rows =
    lifeWaveRemoteInferenceFrontier?.damage_rows_for_life_wave_wearers ?? 0;
  summary.life_wave_remote_inference_active_damage_rows =
    lifeWaveRemoteInferenceFrontier?.active_damage_rows ?? 0;
  summary.life_wave_remote_inference_inactive_damage_rows =
    lifeWaveRemoteInferenceFrontier?.inactive_damage_rows ?? 0;
  summary.life_wave_remote_inference_exact_direct_pairs =
    lifeWaveRemoteInferenceFrontier?.accepted_direct_pair_count ?? 0;
  summary.life_wave_remote_inference_unpaired_external_active_damage_rows =
    lifeWaveRemoteInferenceFrontier?.unpaired_external_active_damage_rows ?? 0;
  summary.life_wave_remote_inference_frontier_complete =
    lifeWaveRemoteInferenceFrontier?.provider_rdps_credit_allowed === true;
  summary.life_wave_trigger_frontier_complete =
    lifeWaveTriggerFrontier?.provider_rdps_credit_allowed === true &&
    lifeWaveRemoteInferenceFrontier?.provider_rdps_credit_allowed === true;
  summary.party_skill_static_frontier_results = partySkillFrontier.skill_results.length;
  summary.party_rogue_entry_static_frontier_results =
    partySkillFrontier.rogue_entry_results.length;
  summary.party_skill_static_frontier_provider_credit_allowed =
    partySkillFrontier.skill_results.filter((entry) => entry.provider_rdps_credit_allowed).length;
  summary.party_rogue_entry_static_frontier_provider_credit_allowed =
    partySkillFrontier.rogue_entry_results
      .filter((entry) => entry.provider_rdps_credit_allowed).length;
  summary.party_skill_static_frontier_complete = partySkillFrontier.complete;
  summary.party_effect_window_frontier_results = partyEffectWindowFrontier.effect_results.length;
  summary.party_effect_window_observed_effects = partyEffectWindowFrontier.effect_results
    .filter((entry) => entry.status_events > 0).length;
  summary.party_effect_window_provider_credit_allowed = partyEffectWindowFrontier.effect_results
    .filter((entry) => entry.provider_rdps_credit_allowed).length;
  summary.party_effect_window_provider_ownership_proven_effects =
    partyEffectWindowFrontier.effect_results
      .filter((entry) => entry.provider_ownership_proven_for_every_status_event).length;
  summary.party_effect_window_frontier_complete = partyEffectWindowFrontier.complete;
  summary.party_effect_window_remote_cast_rows_synthesized =
    partyEffectWindowFrontier.remote_cast_rows_synthesized;
  summary.target_vulnerability_formula_receipts = targetVulnerabilityFormulaReceipt ? 1 : 0;
  summary.imagine_status_attribute_tier_receipts = imagineStatusAttributeTierReceipt ? 1 : 0;
  summary.imagine_status_attribute_exact_tier_occurrences =
    imagineStatusAttributeTierReceipt?.exact_paired_attribute_occurrences ?? 0;
  summary.imagine_status_attribute_unresolved_applications =
    imagineStatusAttributeTierReceipt?.unresolved_applied_status_instances ?? 0;
  summary.imagine_tier_window_counterfactual_input_receipts =
    imagineTierWindowCounterfactualReceipt ? 1 : 0;
  summary.imagine_tier_window_exact_windows =
    imagineTierWindowCounterfactualReceipt?.exact_apply_remove_windows ?? 0;
  summary.imagine_tier_window_complete_inputs =
    imagineTierWindowCounterfactualReceipt?.complete_window_inputs ?? 0;
  summary.imagine_tier_window_retained_damage_actions =
    imagineTierWindowCounterfactualReceipt?.retained_recipient_damage_actions ?? 0;
  summary.imagine_tier_window_counterfactual_damage_deltas = 0;
  summary.imagine_tier_window_provider_credit_allowed = 0;
  summary.fatal_spiral_damage_stage_frontier_receipts =
    fatalSpiralDamageStageReceipt ? 1 : 0;
  summary.fatal_spiral_gap_bounded_damage_memberships =
    fatalSpiralDamageStageReceipt?.audited_damage_event_memberships ?? 0;
  summary.fatal_spiral_gap_safe_ownership_resolved_damage_memberships =
    fatalSpiralDamageStageReceipt?.gap_safe_damage_memberships ?? 0;
  summary.fatal_spiral_gap_safe_third_party_provider_memberships =
    fatalSpiralDamageStageReceipt?.gap_safe_third_party_provider_memberships ?? 0;
  summary.fatal_spiral_gap_safe_provider_self_memberships =
    fatalSpiralDamageStageReceipt?.gap_safe_provider_self_memberships ?? 0;
  summary.fatal_spiral_gap_safe_ownership_unresolved_memberships =
    fatalSpiralDamageStageReceipt?.gap_safe_ownership_unresolved_memberships ?? 0;
  summary.fatal_spiral_controlled_counterfactual_pairs = 0;
  summary.fatal_spiral_provider_credit_allowed = 0;
  summary.fatal_spiral_automatic_integer_candidate_evaluator_receipts =
    fatalSpiralDamageStageReceipt?.automatic_integer_candidate_evaluator_integrated === true
      ? 1 : 0;
  summary.fatal_spiral_automatic_integer_candidate_evaluated_variants =
    fatalSpiralDamageStageReceipt?.automatic_integer_candidate_evaluated_variants ?? 0;
  summary.fatal_spiral_retained_capture_exhaustion_receipts =
    fatalSpiralDamageStageReceipt
      ?.retained_current_build_present_and_absent_capture_frontier_exhausted === true
      ? 1 : 0;
  summary.fatal_spiral_all_reviewed_comparison_samples =
    fatalSpiralDamageStageReceipt?.all_reviewed_comparison_samples ?? 0;
  summary.fatal_spiral_recovered_partial_prefix_exhaustion_receipts =
    fatalSpiralDamageStageReceipt?.retained_recovered_partial_prefix_frontier_exhausted === true
      ? 1 : 0;
  summary.fatal_spiral_recovered_partial_prefix_comparison_samples =
    fatalSpiralDamageStageReceipt?.recovered_partial_prefix_comparison_samples ?? 0;
  summary.fatal_spiral_recovered_partial_prefix_controlled_pairs = 0;
  summary.fatal_spiral_controlled_capture_client_surface_receipts =
    fatalSpiralDamageStageReceipt?.exact_build_hidden_controlled_capture_surface_identified === true
      ? 1 : 0;
  summary.fatal_spiral_currently_executable_controlled_capture_routes = 0;
  summary.fatal_spiral_training_scene_access_frontier_receipts =
    fatalSpiralDamageStageReceipt?.exact_build_training_scene_access_frontier_reviewed === true
      ? 1 : 0;
  summary.fatal_spiral_ordinary_training_scene_entry_routes_proven = 0;
  summary.fatal_spiral_native_immediate_consumer_search_receipts =
    fatalSpiralDamageStageReceipt?.exact_native_immediate_family_search_exhausted === true
      ? 1 : 0;
  summary.fatal_spiral_combat_relevant_exact_immediate_consumers = 0;
  summary.fatal_spiral_generic_getter_call_search_receipts =
    fatalSpiralDamageStageReceipt?.bounded_direct_getter_call_search_exhausted === true
      ? 1 : 0;
  summary.fatal_spiral_combat_relevant_literal_getter_consumers = 0;
  summary.fatal_spiral_exact_pointer_slot_reference_search_receipts =
    fatalSpiralDamageStageReceipt?.exact_rip_relative_slot_reference_search_exhausted === true
      ? 1 : 0;
  summary.fatal_spiral_indexed_metadata_consumers_excluded = 0;
  summary.fatal_spiral_sealed_candidate_readiness_receipts =
    fatalSpiralDamageStageReceipt?.recursive_sealed_rlog_candidate_discovery_bounded === true
      ? 1 : 0;
  summary.fatal_spiral_current_new_sealed_candidate_rlogs =
    fatalSpiralDamageStageReceipt?.current_new_sealed_candidate_rlogs ?? 0;
  summary.fatal_spiral_source_transition_same_context_pairs =
    fatalSpiralDamageStageReceipt?.current_source_transition_same_context_pairs ?? 0;
  summary.fatal_spiral_source_transition_minimum_residual_before_configured_endpoint_diagnostic =
    fatalSpiralDamageStageReceipt
      ?.current_source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic ?? 0;
  summary.fatal_spiral_source_transition_minimum_residual_after_configured_endpoint_diagnostic =
    fatalSpiralDamageStageReceipt
      ?.current_source_transition_minimum_residual_observed_state_dimensions ?? 0;
  summary.fatal_spiral_source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions = 0;
  summary.fatal_spiral_source_transition_strict_controlled_pairs = 0;
  summary.fatal_spiral_configured_endpoint_transition_pairs =
    fatalSpiralDamageStageReceipt?.current_configured_endpoint_transition_pairs ?? 0;
  summary.fatal_spiral_configured_endpoint_transition_residual_ranking_receipts =
    fatalSpiralDamageStageReceipt?.configured_endpoint_transition_residual_ranking_complete === true
      ? 1 : 0;
  summary.fatal_spiral_exact_build_spatial_attribute_identity_receipts =
    fatalSpiralDamageStageReceipt?.exact_build_spatial_attribute_identity_proof_complete === true
      ? 1 : 0;
  summary.fatal_spiral_retained_spatial_raw_value_replay_receipts =
    fatalSpiralDamageStageReceipt?.retained_spatial_raw_value_replay_complete === true
      ? 1 : 0;
  summary.fatal_spiral_spatial_attributes_safe_to_exclude = 0;
  summary.fatal_spiral_exact_build_action_selector_roster_receipts =
    fatalSpiralDamageStageReceipt?.exact_build_action_selector_roster_complete === true
      ? 1 : 0;
  summary.fatal_spiral_exact_action_selectors =
    fatalSpiralDamageStageReceipt?.current_exact_action_selectors ?? 0;
  summary.fatal_spiral_packet_source_route_matched_transition_pairs =
    fatalSpiralDamageStageReceipt?.current_packet_source_route_matched_transition_pairs ?? 0;
  summary.fatal_spiral_packet_source_route_rejected_transition_pairs =
    fatalSpiralDamageStageReceipt?.current_packet_source_route_rejected_transition_pairs ?? 0;
  summary.fatal_spiral_relative_spatial_relation_audit_receipts =
    fatalSpiralDamageStageReceipt?.relative_spatial_relation_audit_complete === true
      ? 1 : 0;
  summary.fatal_spiral_direct_spatial_relation_complete_transition_pairs =
    fatalSpiralDamageStageReceipt?.current_direct_spatial_relation_complete_transition_pairs ?? 0;
  summary.fatal_spiral_direct_spatial_relation_exact_transition_pairs =
    fatalSpiralDamageStageReceipt?.current_direct_spatial_relation_exact_transition_pairs ?? 0;
  summary.fatal_spiral_direct_spatial_relation_nonexact_transition_pairs =
    fatalSpiralDamageStageReceipt?.current_direct_spatial_relation_nonexact_transition_pairs ?? 0;
  summary.fatal_spiral_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs =
    fatalSpiralDamageStageReceipt
      ?.current_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs ?? 0;
  summary.fatal_spiral_fake_bullet_exact_wire_contract_receipts =
    fatalSpiralDamageStageReceipt?.fake_bullet_exact_wire_contract_complete === true ? 1 : 0;
  summary.fatal_spiral_fake_bullet_future_timeline_preservation_receipts =
    fatalSpiralDamageStageReceipt?.fake_bullet_future_capture_timeline_preservation_complete === true
      ? 1 : 0;
  summary.fatal_spiral_fake_bullet_current_build_observed_lifecycle_records =
    fatalSpiralDamageStageReceipt?.fake_bullet_current_build_observed_lifecycle_records ?? 0;
  summary.fatal_spiral_fake_bullet_source4_damage_routes_resolved = 0;
  summary.fatal_spiral_fake_bullet_provider_ownership_proven = 0;
  summary.target_vulnerability_current_build_formulas_proven = 0;
  summary.target_vulnerability_provider_credit_allowed = 0;
  summary.production_runtime_ready = protocolPromotionGate.runtime_promotion_allowed &&
    summary.strict_rdps_proof_complete && summary.support_effect_frontier_complete &&
    summary.party_skill_static_frontier_complete && summary.party_effect_window_frontier_complete &&
    summary.life_wave_trigger_frontier_complete;
  const report = {
    schema_version: PROOF_CLOSURE_SCHEMA_VERSION,
    generated_by: "tools/bpsr-rdps-proof-closure.mjs",
    game_build: context.build,
    policy: {
      matching_build_packet_evidence_required: true,
      external_provider_scope_must_be_observed_not_inferred: true,
      typed_transfer_gate_precedes_packet_relationship_shape: true,
      lifecycle_must_have_apply_and_terminal_event: true,
      ambiguous_removals_or_provider_windows_block_promotion: true,
      required_formula_inputs_must_be_complete: true,
      counterfactual_transfer_gates_always_require_complete_formula_inputs: true,
      counterfactual_projection_must_be_valid_and_conserved: true,
      historical_proofs_are_leads_not_current_build_promotions: true,
      shared_formula_models_collapse_repeated_research: true,
      offline_formula_proof_receipts_never_close_runtime_or_conservation_gates: true,
      canonical_runtime_input_route_proof_receipts_never_close_provider_projection_or_conservation_gates: true,
      shared_proof_receipts_never_close_downstream_runtime_or_conservation_gates: true,
      registry_only_proof_routes_remain_open_without_fabricated_runtime_obligations: true,
      source_rule_ids_are_metadata_not_runtime_selectors: true,
      metadata_only_source_rekeys_require_unique_exact_runtime_selector_contracts: true,
      every_manifest_obligation_is_preserved_exactly_once: true,
      every_packet_observed_runtime_effect_is_preserved_exactly_once: true,
      exact_runtime_replay_may_promote_only_its_conserved_unambiguous_subset: true,
      exact_runtime_subset_promotion_is_not_full_effect_or_family_resolution: true,
      exact_terminal_effect_evidence_enriches_matching_obligations_without_closing_formula_or_projection_gates: true,
      counterfactual_frontier_evidence_never_grants_formula_or_runtime_authority: true,
      provider_ownership_evidence_never_grants_formula_or_runtime_authority: true,
      integer_transform_constraints_never_grant_formula_or_runtime_authority: true,
      component_static_scalar_proofs_never_grant_formula_projection_runtime_or_conservation_authority: true,
      support_effect_proof_receipts_never_grant_opportunity_runtime_or_ui_authority: true,
      life_wave_trigger_receipts_never_grant_formula_runtime_ui_or_credit_authority: true,
      life_wave_remote_inference_receipts_never_grant_exact_formula_runtime_ui_or_credit_authority: true,
      critical_damage_factor_interpretation_receipts_never_grant_formula_runtime_or_ui_authority: true,
      critical_damage_runtime_gate_receipts_never_grant_formula_runtime_or_ui_authority: true,
      exact_build_damage_surface_grouping_never_grants_formula_runtime_or_ui_authority: true,
      bounded_counterfactual_processing_overage_or_missing_pair_never_grants_authority: true,
      provider_formula_context_gaps_are_explicit_blockers: true,
      component_level_current_build_routing_precedes_coarse_runtime_effect_classification: true,
      ambiguous_provider_windows_remain_deferred: true,
      exact_subset_replay_proof_is_not_production_runtime_promotion: true,
      production_runtime_credit_requires_promoted_matching_build_protocol_pack: true,
      protocol_event_coverage_requires_only_locally_observable_exact_routes: true,
      structural_remote_packet_non_obligations_never_synthesize_or_zero_fill_events: true,
      party_skill_static_frontier_never_fabricates_packet_obligations: true,
      party_skill_static_evidence_never_grants_formula_runtime_or_ui_authority: true,
      party_effect_window_evidence_never_assumes_affected_entity_allegiance: true,
      party_effect_window_evidence_never_grants_formula_runtime_or_ui_authority: true,
      party_effect_window_evidence_preserves_both_damage_relationships: true,
      party_effect_window_evidence_preserves_neutral_action_links: true,
      party_effect_packet_origin_edges_are_exact_build_gated_and_non_authoritative: true,
      party_effect_window_evidence_preserves_allegiance_neutral_actor_ability_target_edges: true,
      party_effect_provider_ownership_receipts_require_exact_cohort_and_event_count_match: true,
      party_effect_identity_and_party_membership_gates_are_separate: true,
      party_effect_role_proof_uses_relationship_and_event_time_entity_evidence_not_localized_allegiance: true,
      status_event_season_context_receipts_reject_future_profile_backfill: true,
      status_event_season_context_gap_routes_are_evidence_not_authority: true,
      season_state_mutation_surface_receipts_never_grant_event_time_authority: true,
      static_reconnect_lifecycle_receipts_never_grant_event_time_logout_exclusion: true,
      observed_stacking_frontier_receipts_never_grant_server_arbitration_or_rounding_authority: true,
      native_action_speed_and_conditional_capacity_receipts_never_grant_opportunity_rounding_or_credit_authority: true,
      imagine_static_scalar_receipts_never_grant_event_time_tier_damage_stage_conservation_or_credit_authority: true,
      imagine_status_attribute_tier_receipts_are_occurrence_scoped_and_never_propagate_across_time_or_recipients: true,
      imagine_tier_window_counterfactual_input_receipts_retain_neutral_actions_without_granting_damage_formula_or_credit_authority: true,
      fatal_spiral_damage_stage_frontier_receipts_never_grant_formula_rounding_conservation_runtime_or_ui_authority: true,
      blocked_hidden_damage_control_surfaces_are_acquisition_evidence_not_authorization_or_formula_authority: true,
      unproven_training_scene_routes_never_grant_acquisition_formula_runtime_or_ui_authority: true,
      exact_native_immediate_search_receipts_never_exclude_computed_indirect_table_driven_or_protected_consumers: true,
      generic_getter_and_exact_pointer_slot_receipts_never_exclude_runtime_derived_indexed_metadata_or_protected_consumers: true,
      sealed_candidate_readiness_receipts_never_grant_controlled_pair_formula_runtime_or_ui_authority: true,
      configured_endpoint_attribute_family_diagnostics_never_grant_formula_input_operation_order_runtime_or_ui_authority: true,
      exact_build_spatial_attribute_names_are_evidence_only_and_spatial_dimensions_remain_matched: true,
      exact_build_action_selectors_and_packet_source_routes_never_grant_formula_runtime_or_ui_authority: true,
      packet_source_route_mismatches_remain_visible_and_unresolved: true,
      relative_spatial_relation_tolerances_are_diagnostic_and_never_promotion_rules: true,
      relative_spatial_relation_equality_never_proves_all_spatial_damage_inputs_equal: true,
      future_fake_bullet_timeline_capture_never_backfills_historical_observations_or_provider_ownership: true,
      fake_bullet_aoi_container_never_becomes_provider_without_exact_relation_proof: true,
      recovered_partial_prefix_receipts_never_grant_source_capture_integrity_formula_runtime_or_ui_authority: true,
      historical_target_vulnerability_pairs_never_grant_current_build_formula_runtime_or_ui_authority: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      manifest: fileDescriptor(context.manifest),
      aggregate: optionalFileDescriptor(context.aggregate),
      static_formula_evidence: fileDescriptor(context.staticFormulaEvidence),
      formula_model_workbench: fileDescriptor(context.workbench),
      formula_proof_carry_forward: fileDescriptor(context.carryForward),
      runtime_attribution_evidence: context.runtimeAttributionEvidence
        ? fileDescriptor(context.runtimeAttributionEvidence)
        : null,
      counterfactual_frontier: context.counterfactualFrontier
        ? fileDescriptor(context.counterfactualFrontier)
        : null,
      counterfactual_rollups: counterfactualRollupPaths.map(fileDescriptor),
      provider_ownership_proofs: providerOwnershipProofPaths.map(fileDescriptor),
      status_event_season_context_proofs:
        statusEventSeasonContextProofPaths.map(fileDescriptor),
      season_state_mutation_proof: context.seasonStateMutationProof
        ? fileDescriptor(context.seasonStateMutationProof)
        : null,
      party_haste_stacking_frontier: context.partyHasteStackingFrontier
        ? fileDescriptor(context.partyHasteStackingFrontier)
        : null,
      action_speed_formula_proof: context.actionSpeedFormulaProof
        ? fileDescriptor(context.actionSpeedFormulaProof)
        : null,
      party_haste_capacity_proof: context.partyHasteCapacityProof
        ? fileDescriptor(context.partyHasteCapacityProof)
        : null,
      action_timing_ancestry_proof: context.actionTimingAncestryProof
        ? fileDescriptor(context.actionTimingAncestryProof)
        : null,
      imagine_formula_proof: context.imagineFormulaProof
        ? fileDescriptor(context.imagineFormulaProof)
        : null,
      imagine_status_attribute_tier_proof: context.imagineStatusAttributeTierProof
        ? fileDescriptor(context.imagineStatusAttributeTierProof)
        : null,
      imagine_tier_window_counterfactual_inputs:
        context.imagineTierWindowCounterfactualInputs
          ? fileDescriptor(context.imagineTierWindowCounterfactualInputs)
          : null,
      fatal_spiral_damage_stage_frontier: context.fatalSpiralDamageStageFrontier
        ? fileDescriptor(context.fatalSpiralDamageStageFrontier)
        : null,
      target_vulnerability_formula_proof: context.targetVulnerabilityFormulaProof
        ? fileDescriptor(context.targetVulnerabilityFormulaProof)
        : null,
      integer_transform_constraints: integerTransformConstraintPaths.map(fileDescriptor),
      component_scalar_proofs: componentScalarProofPaths.map(fileDescriptor),
      support_effect_proofs: supportEffectProofPaths.map(fileDescriptor),
      life_wave_trigger_proof: context.lifeWaveTriggerProof
        ? fileDescriptor(context.lifeWaveTriggerProof)
        : null,
      life_wave_remote_inference_proof: context.lifeWaveRemoteInferenceProof
        ? fileDescriptor(context.lifeWaveRemoteInferenceProof)
        : null,
      critical_damage_factor_interpretation_proof:
        context.criticalDamageFactorInterpretationProof
          ? fileDescriptor(context.criticalDamageFactorInterpretationProof)
          : null,
      critical_factor_controlled_pair_discriminant:
        criticalDamageFactorInterpretationProof?.inputs?.controlled_pair_discriminant
          ? structuredClone(
            criticalDamageFactorInterpretationProof.inputs.controlled_pair_discriminant,
          )
          : null,
      runtime_effect_component_routing_proof: fileDescriptor(context.runtimeEffectComponentRoutingProof),
      party_skill_static_closure: fileDescriptor(context.partySkillStaticClosure),
      party_effect_window_audit: fileDescriptor(context.partyEffectWindowAudit),
      protocol_pack_status: context.protocolStatus
        ? fileDescriptor(context.protocolStatus)
        : null,
    },
    summary,
    production_readiness: {
      ...protocolPromotionGate,
      strict_rdps_proof_complete: summary.strict_rdps_proof_complete,
      support_effect_frontier_complete: summary.support_effect_frontier_complete,
      life_wave_trigger_frontier_complete: summary.life_wave_trigger_frontier_complete,
      life_wave_remote_inference_frontier_complete:
        summary.life_wave_remote_inference_frontier_complete,
      party_skill_static_frontier_complete: summary.party_skill_static_frontier_complete,
      party_effect_window_frontier_complete: summary.party_effect_window_frontier_complete,
      production_runtime_ready: summary.production_runtime_ready,
      blockers: uniqueSorted([
        ...protocolPromotionGate.blockers,
        ...(summary.strict_rdps_proof_complete ? [] : ["strict-rdps-proof-incomplete"]),
        ...supportEffectFrontierResults
          .filter((effect) => !effect.provider_rdps_credit_allowed)
          .map((effect) => `support-effect-open:${effect.effect_id}:${effect.mechanic}`),
        ...(lifeWaveTriggerFrontier
          ? (lifeWaveRemoteInferenceFrontier
            ? (lifeWaveTriggerFrontier.provider_rdps_credit_allowed &&
                lifeWaveRemoteInferenceFrontier.provider_rdps_credit_allowed
              ? []
              : ["life-wave-open:2302421:packet-trigger-remote-formula-inference-and-conservation"])
            : ["life-wave-remote-inference-proof-missing"])
          : ["life-wave-trigger-proof-missing"]),
        ...componentScalarFrontierResults
          .filter((effect) => !effect.exact_damage_projection_proven)
          .map((effect) => `component-scalar-open:${effect.effect_id}:counterfactual-projection`),
        ...partySkillFrontier.skill_results
          .filter((entry) => !entry.provider_rdps_credit_allowed)
          .map((entry) => `party-skill-open:${entry.skill_id}:formula-scope-or-runtime-proof`),
        ...partySkillFrontier.rogue_entry_results
          .filter((entry) => !entry.provider_rdps_credit_allowed)
          .map((entry) => `party-entry-open:${entry.entry_id}:formula-scope-or-runtime-proof`),
        ...partyEffectWindowFrontier.effect_results
          .filter((entry) => entry.status_events > 0 && !entry.provider_rdps_credit_allowed)
          .map((entry) =>
            `party-effect-window-open:${entry.effect_id}:${entry.affected_entity_role_proven
              ? "formula-stacking-rounding-conservation"
              : "affected-entity-scope-membership-formula-stacking-rounding"}`
          ),
        ...(targetVulnerabilityFormulaReceipt
          ? ["target-vulnerability-open:55228:exact-current-build-scalar-operation-order-stacking-rounding-conservation"]
          : []),
      ]),
    },
    closure_gates: closureGateDescriptions(),
    shared_model_results: modelResults,
    source_results: sourceResults,
    obligation_results: obligationResults,
    packet_observed_runtime_effect_results: runtimeEffectResults,
    counterfactual_frontier_results: counterfactualFrontierResults,
    component_scalar_frontier_results: componentScalarFrontierResults,
    support_effect_frontier_results: supportEffectFrontierResults,
    life_wave_trigger_frontier: lifeWaveTriggerFrontier,
    life_wave_remote_inference_frontier: lifeWaveRemoteInferenceFrontier,
    party_skill_static_frontier: partySkillFrontier,
    party_effect_window_frontier: partyEffectWindowFrontier,
    target_vulnerability_formula_frontier: targetVulnerabilityFormulaReceipt,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(context.output);
  console.log(
    `rDPS proof closure built for ${context.build}: ${obligationResults.length} packet obligations -> ` +
    `${modelResults.length} shared formula models; ${report.summary.strictly_promotable_obligations} strictly promotable, ` +
    `${report.summary.open_obligations} open, zero hidden omissions in ${Math.round(performance.now() - started)} ms.`,
  );
}

function buildProtocolPromotionGate(status) {
  const requiredProofSuites = ["canonical-replay-conservation", "protocol-event-coverage"];
  if (!status) {
    return {
      protocol_pack_status: "not-provided",
      protocol_pack_identity_present: false,
      protocol_pack_identity_build_matches: false,
      protocol_pack_byte_identical_to_audited_candidate: false,
      required_proof_suites: requiredProofSuites,
      protocol_event_coverage_scope: "locally-observable-exact-routes",
      structural_remote_packet_non_obligations_excluded_from_acquisition: false,
      structural_remote_packet_non_obligation_count: 0,
      structural_remote_packet_non_obligations: [],
      runtime_promotion_allowed: false,
      blockers: ["protocol-pack-status-input-missing"],
    };
  }
  if (![1, 2, 3, 4].includes(Number(status.schema_version)) ||
    status.generated_by !== "tools/bpsr-protocol-pack-status.mjs") {
    throw new Error("Unsupported protocol-pack status schema or generator");
  }
  if (!Array.isArray(status.blockers)) throw new Error("Protocol-pack status blockers must be an array");
  const structuralRoutes = Number(status.schema_version) >= 2
    ? status.audit?.structural_non_obligation_routes
    : [];
  if (Number(status.schema_version) >= 2 &&
    (status.policy
      ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
      status.policy?.structural_non_obligations_are_not_packet_absence_as_zero !== true ||
      status.policy?.structural_non_obligations_never_synthesize_canonical_events !== true ||
      status.policy?.unknown_and_unresolved_canonical_events_are_preserved !== true ||
      !Array.isArray(structuralRoutes) ||
      structuralRoutes.length !== Number(status.audit?.structural_non_obligation_route_count) ||
      structuralRoutes.some((route) =>
        !Number.isSafeInteger(Number(route.service_id)) || Number(route.service_id) <= 0 ||
        !Number.isSafeInteger(Number(route.method_id)) || Number(route.method_id) <= 0 ||
        Number(route.packet_count) !== 0 || Number(route.decoded_records) !== 0 ||
        route.promotion_requirement_satisfied !== true || !String(route.reason ?? "")
      ))) {
    throw new Error("Protocol-pack status structural non-obligation policy is unsafe");
  }
  if (Number(status.schema_version) >= 3) {
    if (status.policy?.statically_exact_unreplayed_routes_remain_opaque !== true) {
      throw new Error("Protocol-pack status lost the fail-closed static-route policy");
    }
    if (Number(status.audit?.schema_version) >= 4) {
      const disposition = String(status.audit?.use_slot_candidate_disposition ?? "");
      const required = status.audit?.use_slot_runtime_decoder_required;
      const satisfied = status.audit?.use_slot_promotion_requirement_satisfied;
      if (Number(status.audit?.exact_world_call_service_id) !== 103198054 ||
        !["opaque", "allowed:world_use_slot_v1"].includes(disposition) ||
        typeof required !== "boolean" || typeof satisfied !== "boolean" ||
        (required && disposition !== "allowed:world_use_slot_v1") ||
        (!required && (disposition !== "opaque" || satisfied !== true))) {
        throw new Error("Protocol-pack status has unsafe World.UseSlot activation accounting");
      }
    }
  }
  const promoted = status.status === "promoted" &&
    status.audit?.promotion_ready === true &&
    status.promoted_pack?.present === true &&
    status.promoted_pack?.build_matches === true &&
    status.promoted_pack?.byte_identical_to_candidate === true &&
    status.blockers.length === 0;
  return {
    protocol_pack_status: String(status.status ?? "unknown"),
    protocol_pack_identity_present: status.promoted_pack?.present === true,
    protocol_pack_identity_build_matches: status.promoted_pack?.build_matches === true,
    protocol_pack_byte_identical_to_audited_candidate:
      status.promoted_pack?.byte_identical_to_candidate === true,
    required_proof_suites: requiredProofSuites,
    protocol_event_coverage_scope: "locally-observable-exact-routes",
    structural_remote_packet_non_obligations_excluded_from_acquisition:
      Number(status.schema_version) >= 2,
    structural_remote_packet_non_obligation_count: structuralRoutes.length,
    structural_remote_packet_non_obligations: structuredClone(structuralRoutes),
    runtime_promotion_allowed: promoted,
    blockers: uniqueSorted([
      ...(status.blockers ?? []),
      ...(promoted ? [] : [`protocol-pack-status:${String(status.status ?? "unknown")}`]),
    ]),
  };
}

function buildSupportEffectFrontierResults(documents, paths, build) {
  const results = documents.map((proof, index) => {
    if ([3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18].includes(Number(proof?.schema_version)) &&
      proof?.generated_by === "rlogs-bpsr-inspiration-proc-attribution-proof") {
      return buildInspirationProcFrontierResult(proof, paths[index], build);
    }
    if ([28, 29].includes(Number(proof?.schema_version)) &&
      proof?.generated_by === "rlogs-bpsr-rdps-status-attribute-proof") {
      return buildPartyHasteCoefficientFrontierResult(proof, paths[index], build);
    }
    if (Number(proof?.schema_version) !== 2 ||
      proof?.generated_by !== "tools/haste-transform-packet-proof.mjs") {
      throw new Error("Unsupported support-effect proof schema or generator");
    }
    requireBuild(proof.game_build, build, "support-effect proof");
    if (proof.policy?.exact_numeric_attribute_ids_are_authoritative !== true ||
      proof.policy?.localized_names_are_runtime_keys !== false ||
      proof.policy?.missing_or_unobserved_values_are_zero !== false ||
      proof.policy?.remote_player_packets_required !== false ||
      proof.policy?.ordinary_damage_is_retained !== true ||
      proof.policy?.client_ui_evaluator_formula_authority !== true ||
      proof.policy?.recording_build_identity_authority !== false ||
      proof.policy?.exact_season_row_selection_for_each_recording !== false ||
      proof.policy?.combat_transform_formula_authority !== false ||
      proof.policy?.gap_bounded_lifecycle_is_formula_authority !== false ||
      proof.policy?.missing_action_start_packets_mean_zero_actions !== false ||
      proof.policy?.hypothetical_extra_actions_or_damage_may_be_invented !== false ||
      proof.policy?.support_effect_causal_authority !== false ||
      proof.policy?.provider_attribution_authority !== false ||
      proof.policy?.runtime_authority !== false ||
      proof.policy?.rdps_ui_display_authority !== false) {
      throw new Error("Support-effect proof policy is unsafe");
    }

    const transform = proof.transform_contract;
    const opportunity = proof.opportunity_contract;
    const summary = proof.summary;
    if (Number(transform?.raw_attribute_id) !== 11120 ||
      Number(transform?.transformed_attribute_id) !== 11930 ||
      transform?.packet_fixed_point_scale !== 10000 ||
      transform?.client_ui_expression !==
        "100 * raw * p3 / (raw * p2 + p1 + min(season_level * p4, p5) + min(role_level * p6, p7))" ||
      !Array.isArray(transform?.transform_rows) || transform.transform_rows.length !== 3) {
      throw new Error("Support-effect Haste transform contract is incomplete");
    }
    const rowThree = transform.transform_rows.find((row) => Number(row.season_id) === 3);
    if (stableStringify(rowThree?.parameters) !== stableStringify([50000, 1, 1, 0, 0, 0, 0]) ||
      Number(summary?.row_3_truncation_exact_delta_batches) <= 0 ||
      Number(summary?.row_3_truncation_exact_delta_batches) !==
        Number(summary?.row_3_truncation_constant_additive_residual_batches) ||
      stableStringify(summary?.row_3_truncation_observed_additive_residuals) !==
        stableStringify([0, 2250])) {
      throw new Error("Support-effect Haste row-3 packet proof is inconsistent");
    }
    if (Number(opportunity?.effect_id) !== 2207252 ||
      opportunity?.evidence_state !==
        "exact-stat-transform-and-gap-bounded-lifecycles-proven-action-opportunity-unobservable" ||
      Number(opportunity?.source_rlogs) <= 0 ||
      Number(opportunity?.canonical_events) <= 0 ||
      Number(opportunity?.status_windows_started) <= 0 ||
      Number(opportunity?.complete_gap_bounded_lifecycles) <= 0 ||
      Number(opportunity?.complete_gap_bounded_lifecycles) >
        Number(opportunity?.status_windows_started) ||
      Number(opportunity?.complete_windows_with_observed_damage) <= 0 ||
      Number(opportunity?.complete_windows_with_observed_damage) >
        Number(opportunity?.complete_gap_bounded_lifecycles) ||
      Number(opportunity?.observed_damage_events_while_active) <= 0 ||
      Number(opportunity?.observed_action_start_events) !== 0 ||
      opportunity?.local_action_start_coverage_observed !== false ||
      opportunity?.zero_action_inference_allowed !== false ||
      opportunity?.extra_action_counterfactual_proven !== false ||
      Number(opportunity?.observed_damage_reassigned_to_provider) !== 0 ||
      opportunity?.formula_authority !== false ||
      opportunity?.runtime_authority !== false ||
      opportunity?.rdps_ui_display_authority !== false ||
      opportunity?.provider_rdps_credit_allowed !== false) {
      throw new Error("Support-effect Haste opportunity contract is unsafe");
    }
    if (Number(summary?.gap_bounded_effect_lifecycles) !==
        Number(opportunity.complete_gap_bounded_lifecycles) ||
      Number(summary?.gap_bounded_effect_windows_with_damage) !==
        Number(opportunity.complete_windows_with_observed_damage) ||
      Number(summary?.observed_damage_events_in_gap_bounded_effect_windows) !==
        Number(opportunity.observed_damage_events_while_active) ||
      Number(summary?.action_start_events_observed) !==
        Number(opportunity.observed_action_start_events)) {
      throw new Error("Support-effect Haste summary does not conserve its opportunity contract");
    }

    return {
      effect_id: String(opportunity.effect_id),
      mechanic: "haste-action-opportunity",
      proof_state: proof.proof_state,
      proof: fileDescriptor(paths[index]),
      exact_stat_transform_proven: true,
      raw_attribute_id: String(transform.raw_attribute_id),
      transformed_attribute_id: String(transform.transformed_attribute_id),
      packet_fixed_point_scale: Number(transform.packet_fixed_point_scale),
      row_3_delta_formula: "trunc_or_floor(10000 * raw_haste / (raw_haste + 50000))",
      exact_delta_batches: Number(summary.row_3_truncation_exact_delta_batches),
      absolute_additive_residuals: structuredClone(
        summary.row_3_truncation_observed_additive_residuals,
      ),
      gap_bounded_lifecycles: Number(opportunity.complete_gap_bounded_lifecycles),
      gap_bounded_windows_with_damage: Number(opportunity.complete_windows_with_observed_damage),
      observed_damage_events_while_active: Number(opportunity.observed_damage_events_while_active),
      observed_action_start_events: Number(opportunity.observed_action_start_events),
      action_start_coverage_observed: false,
      opportunity_counterfactual_proven: false,
      observed_damage_reassigned_to_provider: 0,
      blockers: [
        "exact season-row selection for each recording is unproven",
        "absolute HastePct includes an unresolved additive state",
        "AttackSpeed has a state-dependent unresolved transform",
        "action-start coverage is unavailable and cannot be interpreted as zero actions",
        "additional-action damage counterfactual and conservation are unproven",
      ],
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    };
  });
  uniqueIndex(results, "effect_id", "support-effect frontier result");
  return results.sort((left, right) => compareText(left.effect_id, right.effect_id));
}

function validateLifeWaveTriggerProof(proof, inputPath, build) {
  if (!proof) return null;
  const proofSchemaVersion = Number(proof.schema_version);
  if (![1, 2, 3].includes(proofSchemaVersion) ||
    proof.generated_by !== "tools/bpsr-life-wave-trigger-proof.mjs" ||
    proof.content_sha256 !== orderedContentHash(proof)) {
    throw new Error("Unsupported Life Wave trigger proof schema, generator, or content hash");
  }
  requireBuild(proof.game_build, build, "Life Wave trigger proof");
  const mechanic = proof.mechanic ?? {};
  const staticProof = proof.static_proof ?? {};
  const statusAttribute = proof.status_attribute_proof ?? {};
  const secondaryAttribute = proof.secondary_attribute_proof ?? {};
  const timeline = proof.observed_timeline_proof ?? {};
  const summary = timeline.summary ?? {};
  const selected = proofSchemaVersion >= 2
    ? (timeline.same_capture_packet ?? {})
    : (timeline.selected_window ?? {});
  const conclusion = proof.conclusion ?? {};
  const activationCount = Number(summary.activation_count);
  const selectedCounts = [
    Number(selected.unique_provider_activation_count),
    Number(selected.ambiguous_provider_activation_count),
    Number(selected.no_heal_candidate_activation_count),
  ];
  if (Number(mechanic.module_effect_id) !== 2404 ||
    Number(mechanic.parent_buff_id) !== 2302420 ||
    Number(mechanic.refreshable_window_buff_id) !== 2302421 ||
    Number(mechanic.max_hp_add_attribute_id) !== 11322 ||
    Number(mechanic.linked_damage_id) !== 2230242103 ||
    !String(staticProof.localized_trigger_description ?? "").includes("When HP changes") ||
    Number(staticProof.child_window?.duration_millis) !== 5000 ||
    stableStringify(staticProof.child_window?.destroy_param) !== stableStringify([[0, 5]]) ||
    !Array.isArray(staticProof.eligible_secondary_stat_families) ||
    stableStringify(staticProof.eligible_secondary_stat_families.map((entry) =>
      Number(entry.final_attribute_id))) !== stableStringify([11110, 11120, 11130, 11140, 11150]) ||
    Number(statusAttribute.schema_version) !== 29 ||
    Number(statusAttribute.current_hp?.same_wire_transition_count) <= 0 ||
    Number(statusAttribute.max_hp?.same_wire_transition_count) <= 0 ||
    Number(secondaryAttribute.schema_version) !== 29 ||
    Number(secondaryAttribute.configured_magnitude_observation_count) <= 0 ||
    !Array.isArray(secondaryAttribute.configured_raw_magnitudes) ||
    stableStringify(secondaryAttribute.configured_raw_magnitudes) !== stableStringify([600, 1000]) ||
    !Number.isSafeInteger(activationCount) || activationCount <= 0 ||
    Number(summary.duration_5000_activation_count) !== activationCount ||
    Number(summary.same_instance_reproc_before_expiry_count) <= 0 ||
    (proofSchemaVersion >= 2
      ? (selected.candidate_mode !== "same_capture_packet" ||
        proof.policy?.remote_character_snapshot_required !== false ||
        !String(proof.policy?.remote_recipient_counterfactual ?? "")
          .includes("active/inactive packet outputs") ||
        conclusion.same_capture_packet_trigger_cohort_observed !== true ||
        (proofSchemaVersion >= 3 &&
          (proof.external_formula_crosscheck?.role !==
            "user-supplied-formula-crosscheck-not-standalone-authority" ||
            !/^[0-9a-f]{40}$/.test(String(proof.external_formula_crosscheck?.revision ?? "")) ||
            Number(proof.external_formula_crosscheck?.life_wave_level_5_percentage_points) !== 6 ||
            Number(proof.external_formula_crosscheck?.life_wave_level_6_percentage_points) !== 10 ||
            proof.external_formula_crosscheck?.remote_character_snapshot_required_by_rdps_accounting !== false)))
      : (selected.candidate_mode !== "nearest_preceding_game_time" ||
        Number(selected.window_millis) !== 250)) ||
    selectedCounts.some((value) => !Number.isSafeInteger(value) || value < 0) ||
    selectedCounts.reduce((sum, value) => sum + value, 0) !== activationCount ||
    Number(selected.unique_provider_activation_count) !==
      Number(selected.unique_external_provider_activation_count) +
        Number(selected.unique_self_provider_activation_count) ||
    conclusion.refreshable_five_second_window_proven !== true ||
    conclusion.self_and_external_heal_trigger_candidates_observed !== true ||
    conclusion.current_hp_change_same_wire_with_life_wave_observed !== true ||
    conclusion.max_hp_change_same_wire_with_life_wave_observed !== true ||
    conclusion.configured_secondary_stat_magnitude_observed_in_candidate_lanes !== true ||
    conclusion.max_hp_trigger_provider_observable_in_this_timeline !== false ||
    conclusion.life_wave_secondary_lane_counterfactual_complete !== false ||
    conclusion.runtime_promotion_allowed !== false ||
    proof.policy?.production_promotion_allowed !== false ||
    !Array.isArray(conclusion.remaining_gates) || conclusion.remaining_gates.length === 0) {
    throw new Error("Life Wave trigger proof is incomplete or unsafe");
  }
  for (const key of ["timeline", "status_attribute_proof", "secondary_attribute_proof"]) {
    if (!isValidFileDescriptor(proof.sources?.[key])) {
      throw new Error(`Life Wave trigger proof source ${key} is invalid`);
    }
  }
  return {
    module_effect_id: 2404,
    parent_effect_id: 2302420,
    effect_id: 2302421,
    proof: fileDescriptor(inputPath),
    proof_state:
      proofSchemaVersion >= 2
        ? "exact-build-same-packet-hp-change-refresh-cohort-observed-remote-paired-output-counterfactual-open"
        : "exact-build-hp-change-refresh-window-and-secondary-magnitude-observed-trigger-lane-counterfactual-conservation-open",
    proof_schema_version: proofSchemaVersion,
    activation_count: activationCount,
    same_instance_reproc_before_expiry_count:
      Number(summary.same_instance_reproc_before_expiry_count),
    unique_external_heal_candidate_activations:
      Number(selected.unique_external_provider_activation_count),
    unique_self_heal_candidate_activations:
      Number(selected.unique_self_provider_activation_count),
    ambiguous_heal_candidate_activations:
      Number(selected.ambiguous_provider_activation_count),
    no_heal_candidate_activations:
      Number(selected.no_heal_candidate_activation_count),
    current_hp_same_wire_transitions:
      Number(statusAttribute.current_hp.same_wire_transition_count),
    max_hp_same_wire_transitions:
      Number(statusAttribute.max_hp.same_wire_transition_count),
    configured_secondary_magnitude_observations:
      Number(secondaryAttribute.configured_magnitude_observation_count),
    refresh_timer_reset_proven: true,
    trigger_candidate_basis:
      proofSchemaVersion >= 2 ? "same-capture-packet" : "nearest-preceding-250ms",
    remote_character_snapshot_required: false,
    remote_paired_output_counterfactual_required: proofSchemaVersion >= 2,
    per_refresh_trigger_provider_proven: false,
    selected_secondary_lane_proven: false,
    exact_damage_counterfactual_proven: false,
    exact_integer_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_authority: false,
    provider_rdps_credit_allowed: false,
    blockers: uniqueSorted(conclusion.remaining_gates),
  };
}

function validateLifeWaveRemoteInferenceProof(proof, inputPath, triggerProofPath, build) {
  if (!proof) return null;
  if (!triggerProofPath) {
    throw new Error("Life Wave remote inference proof requires the matching trigger proof");
  }
  if (Number(proof.schema_version) !== 1 ||
    proof.generated_by !== "tools/bpsr-life-wave-remote-inference-proof.mjs" ||
    proof.content_sha256 !== lineTerminatedContentHash(proof)) {
    throw new Error(
      "Unsupported Life Wave remote inference proof schema, generator, or content hash",
    );
  }
  requireBuild(proof.game_build, build, "Life Wave remote inference proof");
  const accounting = proof.accounting_contract ?? {};
  const summary = proof.observed?.summary ?? {};
  const direct = proof.observed?.direct_output?.summary ?? {};
  const occurrence = proof.observed?.occurrence?.summary ?? {};
  const conclusion = proof.conclusion ?? {};
  const triggerDescriptor = fileDescriptor(triggerProofPath);
  const nonNegativeIntegers = [
    summary.activation_count,
    summary.wearer_count,
    summary.damage_rows_for_life_wave_wearers,
    summary.active_damage_rows,
    summary.inactive_damage_rows,
    summary.unique_external_provider_windows,
    summary.unique_self_windows,
    summary.ambiguous_provider_windows,
    summary.no_packet_heal_candidate_windows,
    summary.remote_character_snapshot_rows_consumed,
    direct.unique_external_active_damage_rows,
    direct.accepted_pair_count,
    direct.accepted_marginal_damage,
    direct.accepted_example_count,
    direct.unpaired_unique_external_active_damage_rows,
    occurrence.cohort_count,
    occurrence.cohorts_with_active_and_inactive_samples,
    occurrence.active_samples,
    occurrence.inactive_samples,
  ].map(Number);
  if (Number(proof.effect_id) !== 2302421 ||
    !isValidFileDescriptor(proof.sources?.timeline) ||
    !sameFileContentIdentity(proof.sources?.trigger_proof, triggerDescriptor) ||
    accounting.remote_character_snapshot_required !== false ||
    accounting.remote_loadout_required !== false ||
    accounting.cross_vantage_exact_evidence_preferred !== true ||
    !String(accounting.cross_vantage_join_rule ?? "").includes("exact-instance run group") ||
    !String(accounting.cross_vantage_damage_rule ?? "").includes("never sum duplicate combat events") ||
    !String(accounting.inference_fallback_rule ?? "").includes("no exact same-run observer upload") ||
    accounting.selected_highest_stat_lane_required_for_direct_paired_output !== false ||
    accounting.exact_lane_formula_required_when_no_output_pair_exists !== true ||
    !String(accounting.chance_lane_rule ?? "").includes("occurrence-rate inference") ||
    !String(accounting.haste_lane_rule ?? "").includes("action-opportunity inference") ||
    !String(accounting.conservation ?? "").includes("unchanged ordinary damage") ||
    !String(accounting.conservation ?? "").includes("cannot enter exact rDPS totals") ||
    nonNegativeIntegers.some((value) => !Number.isSafeInteger(value) || value < 0) ||
    Number(summary.activation_count) <= 0 || Number(summary.wearer_count) <= 0 ||
    Number(summary.damage_rows_for_life_wave_wearers) !==
      Number(summary.active_damage_rows) + Number(summary.inactive_damage_rows) ||
    Number(summary.remote_character_snapshot_rows_consumed) !== 0 ||
    Number(direct.unique_external_active_damage_rows) !==
      Number(direct.accepted_pair_count) +
        Number(direct.unpaired_unique_external_active_damage_rows) ||
    Number(direct.accepted_example_count) > Number(direct.accepted_pair_count) ||
    direct.production_exact_authority !== false ||
    occurrence.production_exact_authority !== false ||
    conclusion.remote_snapshot_dependency_removed !== true ||
    conclusion.remote_packet_final_damage_available !== true ||
    conclusion.unique_external_trigger_windows_available !== true ||
    conclusion.runtime_exact_promotion_allowed !== false ||
    conclusion.inferred_display_path_required !== true ||
    !Array.isArray(conclusion.remaining_gates) || conclusion.remaining_gates.length === 0) {
    throw new Error("Life Wave remote inference proof is incomplete or unsafe");
  }
  return {
    effect_id: 2302421,
    proof: fileDescriptor(inputPath),
    proof_state:
      "exact-build-packet-trigger-and-output-census-remote-snapshot-free-formula-inference-conservation-open",
    activation_count: Number(summary.activation_count),
    wearer_count: Number(summary.wearer_count),
    damage_rows_for_life_wave_wearers:
      Number(summary.damage_rows_for_life_wave_wearers),
    active_damage_rows: Number(summary.active_damage_rows),
    inactive_damage_rows: Number(summary.inactive_damage_rows),
    unique_external_provider_windows: Number(summary.unique_external_provider_windows),
    ambiguous_provider_windows: Number(summary.ambiguous_provider_windows),
    no_packet_heal_candidate_windows:
      Number(summary.no_packet_heal_candidate_windows),
    accepted_direct_pair_count: Number(direct.accepted_pair_count),
    unpaired_external_active_damage_rows:
      Number(direct.unpaired_unique_external_active_damage_rows),
    occurrence_cohorts_with_active_and_inactive_samples:
      Number(occurrence.cohorts_with_active_and_inactive_samples),
    remote_character_snapshot_required: false,
    remote_loadout_required: false,
    cross_vantage_exact_evidence_preferred: true,
    inferred_display_path_required: true,
    selected_secondary_lane_proven: false,
    exact_damage_counterfactual_proven: false,
    exact_integer_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_authority: false,
    provider_rdps_credit_allowed: false,
    blockers: uniqueSorted(conclusion.remaining_gates),
  };
}

function buildPartyHasteCoefficientFrontierResult(proof, proofPath, build) {
  requireBuild(proof.expected_game_build, build, "party Haste coefficient proof");
  if (stableStringify(proof.selected_effect_ids) !== stableStringify([31602]) ||
    stableStringify(proof.reported_effect_ids) !== stableStringify([31602]) ||
    stableStringify(proof.selected_attribute_ids) !== stableStringify([
      11120, 11121, 11122, 11123, 11124, 11125,
      11930, 11931, 11932, 11933, 11934, 11935,
    ]) || proof.policy?.session_scope !== "each_rlog_is_processed_independently" ||
    proof.policy?.run_scope !==
      "attributes_statuses_and_owner_links_never_cross_run_ordinals" ||
    proof.policy?.attribute_units !==
      "raw_exact_attribute_units_only_no_percent_flat_or_multiplier_conversion" ||
    proof.policy?.formula_inference !== false ||
    proof.policy?.unresolved_evidence_is_hidden !== false ||
    proof.policy?.selected_attributes_are_formula_context_not_credit_authority !== true) {
    throw new Error("Party Haste coefficient proof policy or selection is unsafe");
  }
  const sessions = proof.sessions ?? [];
  if (sessions.length !== 5 || sessions.some((session) =>
    String(session?.deployment_id) !== "global" ||
    String(session?.game_build) !== String(build) ||
    Number(session?.undecodable_selected_attribute_values) !== 0) ||
    sessions.reduce((sum, session) => sum + Number(session?.selected_status_events ?? 0), 0) !== 130) {
    throw new Error("Party Haste coefficient proof session identity or coverage is incomplete");
  }
  const effect = (proof.effects ?? []).find((entry) => Number(entry?.effect_id) === 31602);
  if ((proof.effects ?? []).length !== 1 || Number(effect?.selected_status_events) !== 130 ||
    Number(effect?.selected_mechanic_state_changes) !== 130) {
    throw new Error("Party Haste coefficient proof effect lifecycle coverage is inconsistent");
  }
  const expectedAttributes = [11930, 11931, 11932];
  const coefficientProofs = proof.reversible_static_coefficient_proofs ?? [];
  if (coefficientProofs.length !== expectedAttributes.length ||
    coefficientProofs.some((entry, index) =>
      Number(entry?.attribute_id) !== expectedAttributes[index] ||
      Number(entry?.fingerprint?.effect_id) !== 31602 ||
      Number(entry?.fingerprint?.origin?.source_type_id) !== 1 ||
      Number(entry?.fingerprint?.origin?.source_config_id) !== 31601 ||
      Number(entry?.fingerprint?.stacks) !== 1 ||
      entry?.status !== "proven_reversible_static_coefficient" ||
      Number(entry?.proven_coefficient_units) !== 1000 ||
      stableStringify(entry?.normalized_coefficient_counts) !== stableStringify({ 1000: 5 }) ||
      Number(entry?.apply_occurrences) !== 3 || Number(entry?.remove_occurrences) !== 2 ||
      Number(entry?.independent_run_contexts) !== 2 ||
      Number(entry?.cross_actor_occurrences) !== 5 ||
      Number(entry?.self_source_occurrences) !== 0 ||
      Number(entry?.missing_source_occurrences) !== 0 ||
      entry?.runtime_eligible_for_rdps !== false)) {
    throw new Error("Party Haste reversible raw coefficient proof is incomplete");
  }
  const lifecycleProofs = proof.matched_lifecycle_coefficient_proofs ?? [];
  if (lifecycleProofs.length !== expectedAttributes.length || lifecycleProofs.some((entry, index) =>
    Number(entry?.attribute_id) !== expectedAttributes[index] ||
    entry?.status !== "insufficient_matched_lifecycle_pairs" ||
    Number(entry?.exact_pair_count) !== 0 || entry?.runtime_eligible_for_rdps !== false)) {
    throw new Error("Party Haste unmatched lifecycle evidence was not preserved");
  }
  return {
    effect_id: "31602",
    mechanic: "party-haste-percent-status-coefficient",
    proof_state: "exact-current-build-effect-to-raw-haste-percent-family-coefficient-proven-downstream-semantics-open",
    proof_schema_version: Number(proof.schema_version),
    proof: fileDescriptor(proofPath),
    exact_stat_transform_proven: true,
    changed_attribute_ids: expectedAttributes.map(String),
    exact_raw_additive_coefficient_units: 1000,
    apply_occurrences: 3,
    remove_occurrences: 2,
    independent_run_contexts: 2,
    cross_actor_occurrences: 5,
    missing_source_occurrences: 0,
    exact_origin: { source_type_id: 1, source_config_id: 31601 },
    raw_unit_interpretation_authority: false,
    stacking_arbitration_proven: false,
    opportunity_counterfactual_proven: false,
    observed_damage_reassigned_to_provider: 0,
    blockers: [
      "event-time party membership for external affected entities is unproven",
      "raw HastePct-family fixed-point interpretation is not independently promoted by this receipt",
      "multi-provider stacking arbitration and refresh order are unproven",
      "action-frequency or cooldown opportunity formula and downstream integer rounding are unproven",
      "additional-action damage counterfactual and conservation are unproven",
    ],
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function attachCriticalDamageFactorInterpretationProof(results, proof, proofPath, build) {
  const inspiration = results.find((entry) => entry.effect_id === "2202041") ?? null;
  const requiresCompanion = Number(inspiration?.proof_schema_version ?? 0) >= 17;
  if (requiresCompanion && (!proof || !proofPath)) {
    throw new Error(
      "Inspiration schema 17 requires an exact-build critical-damage factor interpretation proof",
    );
  }
  if (!proof && !proofPath) return;
  if (!requiresCompanion) {
    throw new Error(
      "Critical-damage factor interpretation proof requires an Inspiration schema-17 support proof",
    );
  }
  requireFile(proofPath, "critical-damage factor interpretation proof");
  requireBuild(proof.game_build, build, "critical-damage factor interpretation proof");
  if (Number(proof.schema_version) !== 4 ||
    proof.generated_by !== "tools/bpsr-critical-damage-factor-interpretation-proof.mjs" ||
    proof.proof_state !==
      "exact-current-build-critical-damage-family-identity-and-local-sync-scope-proven-damage-factor-interpretation-open" ||
    proof.content_sha256 !== orderedContentHash(proof)) {
    throw new Error("Critical-damage factor interpretation proof identity is invalid");
  }
  const policy = proof.policy ?? {};
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
    throw new Error("Critical-damage factor interpretation proof policy is unsafe");
  }
  const family = proof.critical_damage_attribute_family ?? {};
  if (stableStringify([
    family.current_attribute_id, family.total_attribute_id, family.add_attribute_id,
    family.extra_add_attribute_id, family.percent_attribute_id,
    family.extra_percent_attribute_id,
  ]) !== stableStringify([12510, 12511, 12512, 12513, 12514, 12515]) ||
    family.numeric_type !== 1 || family.storage_type !== "int32" ||
    family.base_attribute_value !== 0 || family.sync_to_local_player !== true ||
    family.sync_to_area_of_interest !== false ||
    family.exact_static_sync_scope !== "local-player-only-not-AOI" ||
    family.names_are_runtime_keys !== false ||
    family.damage_consumer_semantics_proven !== false) {
    throw new Error("Critical-damage attribute family receipt is incomplete");
  }
  if (proof.client_consumer_boundary?.current_client_damage_factor_operator_present !== false ||
    proof.client_consumer_boundary?.server_operation_order_and_integer_rounding_proven !== false ||
    proof.client_consumer_boundary?.runtime_formula_authority !== false ||
    proof.interpretation_resolution?.authoritative_interpretation !== null ||
    stableStringify(proof.interpretation_resolution?.retained_candidates) !==
      stableStringify(["additive_bonus", "direct_total"]) ||
    proof.interpretation_resolution?.formula_authority !== false ||
    proof.historical_lead?.current_build_formula_authority !== false ||
    proof.historical_lead?.substitution_into_current_build_allowed !== false ||
    proof.production_gates?.ready_for_snapshot !== false ||
    proof.production_gates?.runtime_promotion_allowed !== false ||
    proof.production_gates?.missing_required_input?.id !== "protocol-pack-identity" ||
    stableStringify(proof.production_gates?.required_proof_suites) !==
      stableStringify(["canonical-replay-conservation", "protocol-event-coverage"]) ||
    proof.runtime_decision?.provider_rdps_credit_allowed !== false ||
    proof.runtime_decision?.runtime_catalog_promotion_allowed !== false ||
    proof.runtime_decision?.ui_rdps_display_allowed !== false ||
    proof.runtime_decision?.ordinary_damage_totals_unchanged !== true) {
    throw new Error("Critical-damage interpretation receipt promoted unresolved evidence");
  }
  const runtimeGate = proof.runtime_interpretation_gate ?? {};
  if (Number(runtimeGate.runtime_schema_version) !== 5 ||
    stableStringify(runtimeGate.promotion_blockers) !== stableStringify([
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
    stableStringify(runtimeGate.retained_candidate_arithmetic_implemented) !==
      stableStringify(["additive_bonus", "direct_total"]) ||
    runtimeGate.candidate_arithmetic_formula_authority !== false) {
    throw new Error("Critical-damage runtime interpretation gate receipt is unsafe");
  }
  const cohortInput = proof.inputs?.inspiration_cohort_proof;
  if (!String(cohortInput?.path ?? "") || !Number.isSafeInteger(Number(cohortInput?.bytes)) ||
    Number(cohortInput.bytes) <= 0 ||
    !/^[0-9a-f]{64}$/.test(String(cohortInput?.sha256 ?? "")) ||
    Number(cohortInput.bytes) !== Number(inspiration.proof.bytes) ||
    String(cohortInput.sha256) !== String(inspiration.proof.sha256)) {
    throw new Error("Critical-damage interpretation receipt is not bound to the Inspiration cohort");
  }
  const cohort = proof.current_build_cohort ?? {};
  if (Number(cohort.exact_rlogs) !== Number(inspiration.exact_rlogs) ||
    Number(cohort.critical_stage_events) !== Number(inspiration.integer_stage_critical_events) ||
    Number(cohort.events_with_compatible_interpretation) !==
      Number(inspiration.integer_stage_events_with_compatible_candidates) ||
    Number(cohort.events_without_compatible_interpretation) !==
      Number(inspiration.integer_stage_events_without_compatible_candidates) ||
    Number(cohort.interpretation_stable_exact_counterfactual_events) !==
      Number(inspiration.integer_stage_exact_counterfactual_events) ||
    Number(cohort.unresolved_order_rounding_or_interpretation_events) !==
      Number(inspiration.integer_stage_unresolved_order_or_rounding_events) ||
    stableStringify(cohort.interpretation_breakdown) !==
      stableStringify(inspiration.critical_factor_interpretation_breakdown) ||
    stableStringify(cohort.candidate_family) !==
      stableStringify(inspiration.integer_stage_counterfactual_coverage?.candidate_family) ||
    cohort.formula_authority !== false) {
    throw new Error("Critical-damage interpretation receipt does not conserve the cohort evidence");
  }
  const controlledPair = proof.controlled_pair_discriminant ?? {};
  const controlledPairInput = proof.inputs?.controlled_pair_discriminant ?? null;
  if (!isValidControlledPairDiscriminantAudit(controlledPair, 2) ||
    !isValidFileDescriptor(controlledPairInput)) {
    throw new Error("Critical-damage controlled-pair discriminant receipt is missing or unsafe");
  }
  inspiration.critical_damage_factor_interpretation_proof = fileDescriptor(proofPath);
  inspiration.critical_factor_controlled_pair_discriminant =
    structuredClone(controlledPairInput);
  inspiration.critical_factor_controlled_pair_audit = structuredClone(controlledPair);
  inspiration.critical_damage_attribute_family = structuredClone(family);
  inspiration.critical_damage_exact_static_sync_scope = family.exact_static_sync_scope;
  inspiration.remote_recipient_attribute_snapshots_required = false;
  inspiration.remote_player_cast_packets_required = false;
  inspiration.remote_player_cast_packets_treated_as_zero = false;
  inspiration.remote_player_cast_packets_synthesized = false;
  inspiration.current_player_attribute_snapshots_substituted_for_remote_players = false;
  inspiration.current_client_damage_factor_operator_present = false;
  inspiration.historical_critical_damage_formula_substitution_allowed = false;
  inspiration.critical_damage_runtime_interpretation_gate = structuredClone(runtimeGate);
  inspiration.blockers = uniqueSorted([
    ...inspiration.blockers,
    "exact-build critical-damage family is local-player-only static sync evidence and does not create remote-recipient snapshots",
    "current client lacks the authoritative server critical-damage operator, operation order, and integer rounding",
    "current runtime critical-damage interpretation is explicitly unresolved and blocks critical-dependent projection",
    `${Number(controlledPair.derived_candidate_pairs)} same-build candidate critical-damage pairs are retained, but none has local event-time state plus complete attack, mitigation, surface, and owner-stage authority`,
  ]);
}

function isValidControlledPairDiscriminantAudit(audit, expectedSchema = 2) {
  if (expectedSchema === 1) {
    return Number(audit?.proof_schema_version) === 1 &&
      Number(audit?.eligible_controlled_pairs) === 0 &&
      Number(audit?.multiple_event_identity_groups) === 0 &&
      Number(audit?.multiple_critical_damage_value_groups) === 0 &&
      Number(audit?.all_stage_inputs_same_wire_groups) === 0 &&
      Number(audit?.zero_age_stage_input_groups) === 0 &&
      Number.isSafeInteger(Number(audit?.exact_surface_rows_with_selected_coefficients)) &&
      Number(audit.exact_surface_rows_with_selected_coefficients) >= 0 &&
      audit?.blocker === "no-same-build-local-event-time-controlled-pairs-retained" &&
      audit?.authoritative_interpretation === null &&
      audit?.formula_authority === false;
  }
  return expectedSchema === 2 && Number(audit?.proof_schema_version) === 2 &&
    Number(audit?.eligible_controlled_pairs) === 0 &&
    Number.isSafeInteger(Number(audit?.derived_candidate_pairs)) &&
    Number(audit.derived_candidate_pairs) > 0 &&
    Number.isSafeInteger(Number(audit?.multiple_event_identity_groups)) &&
    Number(audit.multiple_event_identity_groups) > 0 &&
    Number.isSafeInteger(Number(audit?.multiple_critical_damage_value_groups)) &&
    Number(audit.multiple_critical_damage_value_groups) > 0 &&
    Number(audit?.all_stage_inputs_same_wire_groups) === 0 &&
    Number(audit?.zero_age_stage_input_groups) === 0 &&
    Number(audit?.local_event_time_state_authority_groups) === 0 &&
    Number(audit?.complete_attack_mitigation_preimage_groups) === 0 &&
    Number(audit?.exact_surface_owner_stage_authority_groups) === 0 &&
    Number.isSafeInteger(Number(audit?.exact_surface_rows_with_selected_coefficients)) &&
    Number(audit.exact_surface_rows_with_selected_coefficients) >= 0 &&
    audit?.blocker ===
      "candidate-pairs-retained-but-local-event-time-or-complete-preimage-authority-missing" &&
    audit?.authoritative_interpretation === null &&
    audit?.formula_authority === false;
}

function buildInspirationProcFrontierResult(proof, proofPath, build) {
  if (![3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18].includes(Number(proof.schema_version))) {
    throw new Error("Unsupported Inspiration proc support-effect proof schema");
  }
  requireBuild(proof.game_build, build, "Inspiration proc support-effect proof");
  const policy = proof.policy ?? {};
  if (proof.deployment_id !== "global" || Number(proof.effect_id) !== 2202041 ||
    (Number(proof.schema_version) >= 8
      ? ![28, 29].includes(Number(proof.transition_proof_schema_version))
      : Number(proof.transition_proof_schema_version) !== 27) ||
    policy.remote_player_packets_required !== false ||
    policy.remote_player_packets_treated_as_zero !== false ||
    policy.remote_player_packets_synthesized !== false ||
    (Number(proof.schema_version) >= 5 &&
      policy.unobserved_effect_levels_interpolated !== false) ||
    (Number(proof.schema_version) >= 6 &&
      policy.unobserved_or_mismatched_parent_origins_attributed !== false) ||
    policy.historical_or_current_snapshot_substitution_allowed !== false ||
    (Number(proof.schema_version) >= 9 &&
      (policy.last_observed_local_attribute_is_event_time_formula_authority !== false ||
        policy.snapshot_age_threshold_is_formula_authority !== false ||
        policy.formula_input_snapshot_authority !== false)) ||
    (Number(proof.schema_version) >= 11 &&
      (policy.integer_stage_candidate_family_authority !== false ||
        policy.integer_stage_counterfactual_authority !== false)) ||
    (Number(proof.schema_version) >= 12 &&
      (policy.damage_formula_surface_runtime_authority !== false ||
        policy.ambiguous_damage_surface_rows_inferred !== false ||
        policy.owner_stage_array_selection_is_formula_authority !== false)) ||
    (Number(proof.schema_version) >= 13 &&
      (policy.missing_hit_event_id_may_be_synthesized !== false ||
        policy.unique_ability_damage_surface_candidate_authority !== false)) ||
    (Number(proof.schema_version) >= 14 &&
      policy.damage_script_preimage_breakdown_authority !== false) ||
    (Number(proof.schema_version) >= 15 &&
      policy.damage_surface_identity_groups_include_exact_stage_inputs !== true) ||
    (Number(proof.schema_version) >= 16 &&
      (policy.damage_surface_identity_groups_include_exact_stage_input_freshness !== true ||
        policy.stage_input_freshness_breakdown_authority !== false)) ||
    (Number(proof.schema_version) >= 17 &&
      policy.critical_damage_raw_interpretation_authority !== false) ||
    policy.partial_effect_credit_may_be_displayed_as_complete_effect_rdps !== false ||
    policy.formula_authority !== false || policy.full_effect_composition_authority !== false ||
    policy.canonical_conservation_replay_authority !== false ||
    policy.provider_rdps_credit_authorized !== false ||
    policy.runtime_promotion_allowed !== false || policy.ui_display_allowed !== false ||
    policy.unresolved_evidence_is_hidden !== false) {
    throw new Error("Inspiration proc support-effect proof policy is unsafe");
  }
  const rlogs = Number(proof.schema_version) >= 4 ? proof.rlogs : [proof.rlog];
  const protocolPackDigests = Number(proof.schema_version) >= 4
    ? proof.protocol_pack_digests
    : [proof.protocol_pack_digest];
  if (!Array.isArray(rlogs) || rlogs.length === 0 ||
    !Array.isArray(protocolPackDigests) || protocolPackDigests.length === 0 ||
    protocolPackDigests.some((digest) => !/^sha256:[0-9a-f]{64}$/.test(String(digest)))) {
    throw new Error("Inspiration proc protocol-pack digest is invalid");
  }
  for (const [label, input] of [
    ["transition proof", proof.transition_proof],
    ...(Number(proof.schema_version) >= 12
      ? [["damage formula surface", proof.damage_formula_surface]]
      : []),
    ...rlogs.map((input, index) => [`RLOG ${index + 1}`, input]),
  ]) {
    if (!String(input?.path ?? "") || !Number.isSafeInteger(Number(input?.bytes)) ||
      Number(input.bytes) <= 0 || !/^[0-9a-f]{64}$/.test(String(input?.sha256 ?? ""))) {
      throw new Error(`Inspiration proc ${label} provenance is incomplete`);
    }
  }
  const magnitudes = proof.exact_removal_magnitudes ?? [];
  const magnitudeKeys = new Set();
  const magnitudeLevels = new Map();
  for (const magnitude of magnitudes) {
    const level = Number(magnitude.level);
    const key = [
      magnitude.session_id ?? "single-session-v3",
      magnitude.run_ordinal,
      magnitude.target_entity_uuid,
      magnitude.provider_entity_uuid,
      magnitude.instance_id,
      ...(Number(proof.schema_version) >= 5 ? [level] : []),
      ...(Number(proof.schema_version) >= 6
        ? [magnitude.origin_source_type_id, magnitude.origin_source_config_id]
        : []),
    ].map(String).join(":");
    if (magnitudeKeys.has(key) || Number(magnitude.critical_raw_delta) <= 0 ||
      Number(magnitude.lucky_raw_delta) <= 0 ||
      (Number(proof.schema_version) >= 5 &&
        (!Number.isSafeInteger(level) || level <= 0 ||
          Number(magnitude.critical_raw_delta) !== Number(magnitude.lucky_raw_delta))) ||
      (Number(proof.schema_version) >= 6 &&
        (Number(magnitude.origin_source_type_id) !== 1 ||
          Number(magnitude.origin_source_config_id) !== 2202040)) ||
      Number(magnitude.provider_entity_uuid) === Number(magnitude.target_entity_uuid)) {
      throw new Error("Inspiration proc exact removal magnitude identity is unsafe");
    }
    magnitudeKeys.add(key);
    if (Number(proof.schema_version) >= 5) {
      const entry = magnitudeLevels.get(level) ?? {
        rawDeltas: new Set(), sessions: new Set(), exactInstances: 0,
      };
      entry.rawDeltas.add(Number(magnitude.critical_raw_delta));
      entry.sessions.add(String(magnitude.session_id));
      entry.exactInstances += 1;
      magnitudeLevels.set(level, entry);
    }
  }
  if ([...magnitudeLevels.values()].some((entry) => entry.rawDeltas.size !== 1)) {
    throw new Error("Inspiration proc has conflicting exact magnitudes within one effect level");
  }
  const counts = proof.counts ?? {};
  const blockedCriticalInterpretation =
    Number(counts.candidates_blocked_critical_damage_interpretation ?? 0);
  const emitted = Number(counts.emitted_critical_contributions ?? 0) +
    Number(counts.emitted_lucky_contributions ?? 0) +
    Number(counts.emitted_combined_contributions ?? 0);
  const bucketEvents = (proof.contribution_buckets ?? []).reduce(
    (sum, bucket) => sum + Number(bucket.events ?? 0), 0,
  );
  const unsafeBucket = (proof.contribution_buckets ?? []).some((bucket) =>
    !["critical_proc_bonus", "lucky_proc_occurrence",
      "combined_lucky_occurrence_and_critical_bonus"].includes(String(bucket.path)) ||
    !/^-?\d+$/.test(String(bucket.numerator ?? "")) ||
    !/^\d+$/.test(String(bucket.denominator ?? "")) || BigInt(bucket.denominator) <= 0n ||
    !Number.isSafeInteger(Number(bucket.events)) || Number(bucket.events) <= 0
  );
  const conservation = proof.conservation ?? {};
  if (magnitudes.length === 0 || Number(counts.proof_complete_magnitude_keys) !== magnitudes.length ||
    Number(counts.proof_conflicting_magnitude_keys) !== 0 ||
    Number(counts.status_activation_rows_with_exact_packet_duration) <= 0 ||
    Number(counts.status_activation_rows_without_exact_packet_duration) !== 0 ||
    Number(counts.candidates_with_exact_window_magnitude) !==
      (Number(proof.schema_version) >= 18
        ? emitted + blockedCriticalInterpretation
        : emitted) ||
    (Number(proof.schema_version) >= 18 &&
      (blockedCriticalInterpretation <= 0 ||
        Number(counts.emitted_critical_contributions) !== 0 ||
        Number(counts.emitted_combined_contributions) !== 0)) ||
    emitted <= 0 ||
    Number(counts.candidates_with_multiple_exact_provider_windows) !== 0 ||
    Number(counts.rejected_provider_delta_exceeds_current_chance) !== 0 ||
    Number(counts.rejected_arithmetic_overflow) !== 0 || unsafeBucket || bucketEvents !== emitted ||
    Number(conservation.emitted_contribution_events) !== emitted ||
    Number(conservation.contribution_bucket_events) !== emitted ||
    Number(conservation.unique_emitted_damage_events) !== emitted ||
    Number(conservation.duplicate_emitted_damage_events) !== 0 ||
    conservation.contribution_buckets_match_emitted_events !== true ||
    conservation.every_emitted_damage_event_has_exactly_one_provider_window !== true ||
    conservation.ordinary_damage_totals_mutated !== false ||
    conservation.floating_point_total_is_authoritative !== false) {
    throw new Error("Inspiration proc exact-subset conservation receipt is inconsistent");
  }

  const magnitudeSessions = new Set(
    magnitudes.map((magnitude) => String(magnitude.session_id ?? "single-session-v3")),
  );
  const magnitudeRawDeltas = [...new Set(magnitudes.flatMap((magnitude) => [
    Number(magnitude.critical_raw_delta),
    Number(magnitude.lucky_raw_delta),
  ]))].sort((left, right) => left - right);
  const magnitudeByLevel = [...magnitudeLevels.entries()]
    .sort(([left], [right]) => left - right)
    .map(([level, entry]) => ({
      level,
      raw_delta: [...entry.rawDeltas][0],
      exact_instances: entry.exactInstances,
      independent_sessions: entry.sessions.size,
    }));
  const levelLifecycleEvidence = Number(proof.schema_version) >= 7
    ? proof.level_lifecycle_evidence
    : [];
  if (Number(proof.schema_version) >= 7) {
    if (!Array.isArray(levelLifecycleEvidence) ||
      levelLifecycleEvidence.length !== magnitudeByLevel.length) {
      throw new Error("Inspiration proc per-level lifecycle coverage is incomplete");
    }
    const rawDeltaByLevel = new Map(
      magnitudeByLevel.map((row) => [Number(row.level), Number(row.raw_delta)]),
    );
    const seenLevels = new Set();
    for (const levelRow of levelLifecycleEvidence) {
      const level = Number(levelRow?.level);
      const rawDelta = Number(levelRow?.exact_instance_raw_delta);
      if (!Number.isSafeInteger(level) || level <= 0 || seenLevels.has(level) ||
        rawDeltaByLevel.get(level) !== rawDelta ||
        !Array.isArray(levelRow?.attributes) || levelRow.attributes.length !== 2) {
        throw new Error("Inspiration proc per-level lifecycle identity is inconsistent");
      }
      seenLevels.add(level);
      const attributeIds = levelRow.attributes
        .map((attribute) => Number(attribute?.attribute_id))
        .sort((left, right) => left - right);
      if (JSON.stringify(attributeIds) !== JSON.stringify([11712, 11782])) {
        throw new Error("Inspiration proc per-level lifecycle attributes are incomplete");
      }
      for (const attribute of levelRow.attributes) {
        const countFields = [
          "matching_reversible_rows", "apply_occurrences", "remove_occurrences",
          "independent_run_contexts", "cross_actor_occurrences",
          "matching_lifecycle_rows", "exact_pair_count", "contradictory_pair_count",
          "ambiguous_instance_count", "application_only_instance_count",
          "removal_only_instance_count", "matched_independent_run_contexts",
          "cross_actor_exact_pairs",
        ];
        if (countFields.some((field) =>
          !Number.isSafeInteger(Number(attribute?.[field])) || Number(attribute[field]) < 0) ||
          !Array.isArray(attribute?.reversible_statuses) ||
          !Array.isArray(attribute?.matched_statuses)) {
          throw new Error("Inspiration proc lifecycle counters are invalid");
        }
        const normalizedCounts = attribute?.normalized_coefficient_counts ?? {};
        const exactCounts = attribute?.exact_coefficient_counts ?? {};
        for (const counts of [normalizedCounts, exactCounts]) {
          if (Array.isArray(counts) || typeof counts !== "object" || counts === null ||
            Object.entries(counts).some(([coefficient, count]) =>
              !/^-?\d+$/.test(coefficient) ||
              !Number.isSafeInteger(Number(count)) || Number(count) <= 0)) {
            throw new Error("Inspiration proc lifecycle coefficient counts are invalid");
          }
        }
        const normalizedTotal = Object.values(normalizedCounts)
          .reduce((sum, count) => sum + Number(count), 0);
        const exactTotal = Object.values(exactCounts)
          .reduce((sum, count) => sum + Number(count), 0);
        if (normalizedTotal !== Number(attribute.apply_occurrences) +
            Number(attribute.remove_occurrences) ||
          exactTotal !== Number(attribute.exact_pair_count)) {
          throw new Error("Inspiration proc lifecycle occurrence totals are inconsistent");
        }
        const observedCoefficients = [...new Set([
          ...Object.keys(normalizedCounts), ...Object.keys(exactCounts),
        ].map(Number))];
        const coefficientConsistent = observedCoefficients.length === 1 &&
          observedCoefficients[0] === rawDelta;
        const reversibleGate = Number(attribute.matching_reversible_rows) === 1 &&
          attribute.reversible_statuses.length === 1 &&
          attribute.reversible_statuses[0] === "proven_reversible_static_coefficient" &&
          Number(attribute.reversible_proven_coefficient_units) === rawDelta &&
          Number(attribute.apply_occurrences) > 0 && Number(attribute.remove_occurrences) > 0 &&
          Number(attribute.independent_run_contexts) >= 2 && coefficientConsistent;
        const matchedGate = Number(attribute.matching_lifecycle_rows) === 1 &&
          attribute.matched_statuses.length === 1 &&
          attribute.matched_statuses[0] === "proven_matched_lifecycle_coefficient" &&
          Number(attribute.matched_proven_coefficient_units) === rawDelta &&
          Number(attribute.exact_pair_count) >= 2 &&
          Number(attribute.contradictory_pair_count) === 0 && coefficientConsistent;
        if (Number(proof.schema_version) >= 8) {
          const examples = attribute?.matched_examples;
          if (!Array.isArray(examples)) {
            throw new Error("Inspiration proc matched-lifecycle examples are missing");
          }
          const classifications = new Set();
          for (const example of examples) {
            const applications = example?.applications;
            const removals = example?.removals;
            const classification = String(example?.classification ?? "");
            const fingerprint = example?.fingerprint;
            if (!Array.isArray(applications) || !Array.isArray(removals) ||
              !["exact-opposite-pair", "contradictory-pair", "application-only",
                "removal-only", "ambiguous-multiplicity"].includes(classification) ||
              Number(example?.effect_id) !== 2202041 ||
              Number(fingerprint?.effect_id) !== 2202041 || Number(fingerprint?.level) !== level ||
              Number(fingerprint?.origin?.source_type_id) !== 1 ||
              Number(fingerprint?.origin?.source_config_id) !== 2202040 ||
              fingerprint?.part_id !== null || Number(fingerprint?.stacks) !== 1 ||
              Number(fingerprint?.count) !== -1) {
              throw new Error("Inspiration proc matched-lifecycle example identity is unsafe");
            }
            for (const transition of [...applications, ...removals]) {
              if (!Number.isSafeInteger(Number(transition?.wire_capture_sequence)) ||
                Number(transition.wire_capture_sequence) <= 0 ||
                !Number.isSafeInteger(Number(transition?.before_value)) ||
                !Number.isSafeInteger(Number(transition?.after_value)) ||
                Number(transition.raw_attribute_delta) !==
                  Number(transition.after_value) - Number(transition.before_value)) {
                throw new Error("Inspiration proc matched-lifecycle transition example is invalid");
              }
            }
            const exactOpposite = applications.length === 1 && removals.length === 1 &&
              Number(applications[0].raw_attribute_delta) === -Number(removals[0].raw_attribute_delta);
            const shapeValid = classification === "exact-opposite-pair" ? exactOpposite
              : classification === "contradictory-pair"
                ? applications.length === 1 && removals.length === 1 && !exactOpposite
                : classification === "application-only"
                  ? applications.length === 1 && removals.length === 0
                  : classification === "removal-only"
                    ? applications.length === 0 && removals.length === 1
                    : applications.length + removals.length > 0;
            if (!shapeValid) {
              throw new Error("Inspiration proc matched-lifecycle example classification is invalid");
            }
            classifications.add(classification);
          }
          for (const [countField, classification] of [
            ["exact_pair_count", "exact-opposite-pair"],
            ["contradictory_pair_count", "contradictory-pair"],
            ["application_only_instance_count", "application-only"],
            ["removal_only_instance_count", "removal-only"],
            ["ambiguous_instance_count", "ambiguous-multiplicity"],
          ]) {
            if (Number(attribute[countField]) > 0 && !classifications.has(classification)) {
              throw new Error("Inspiration proc matched-lifecycle example class is not represented");
            }
          }
        }
        if (attribute.coefficient_consistent_with_instance_magnitude !== coefficientConsistent ||
          attribute.reversible_static_gate_passed !== reversibleGate ||
          attribute.matched_lifecycle_gate_passed !== matchedGate) {
          throw new Error("Inspiration proc lifecycle gate flags are inconsistent");
        }
      }
      const staticTransformProven = levelRow.attributes.every((attribute) =>
        attribute.reversible_static_gate_passed === true &&
        attribute.matched_lifecycle_gate_passed === true &&
        attribute.coefficient_consistent_with_instance_magnitude === true
      );
      if (levelRow.reversible_static_transform_proven !== staticTransformProven ||
        !Array.isArray(levelRow?.blockers) ||
        (staticTransformProven ? levelRow.blockers.length !== 0 : levelRow.blockers.length === 0)) {
        throw new Error("Inspiration proc per-level static-transform disposition is inconsistent");
      }
    }
  }
  const reversibleStaticLevelsProven = levelLifecycleEvidence.filter(
    (row) => row.reversible_static_transform_proven === true,
  ).length;
  const formulaInputSnapshotCoverage = Number(proof.schema_version) >= 9
    ? proof.formula_input_snapshot_coverage
    : null;
  let completeFormulaInputEvents = 0;
  let allWireFormulaInputEvents = 0;
  let allNotAfterFormulaInputEvents = 0;
  let maximumFormulaInputAgeMicros = null;
  if (Number(proof.schema_version) >= 9) {
    const pathRows = formulaInputSnapshotCoverage?.paths;
    const attributeRows = formulaInputSnapshotCoverage?.attributes;
    const oldestExamples = formulaInputSnapshotCoverage?.oldest_observed_examples;
    const missingExamples = formulaInputSnapshotCoverage?.missing_examples;
    if (formulaInputSnapshotCoverage?.scope !==
        "exact_single_locally_observed_player_provider_window_with_exact_instance_magnitude_before_formula_evaluation" ||
      formulaInputSnapshotCoverage?.event_time_snapshot_authority !== false ||
      Number(formulaInputSnapshotCoverage?.exact_single_provider_candidate_events) !==
        (Number(proof.schema_version) >= 18
          ? Number(counts.candidates_with_exact_window_magnitude)
          : emitted) ||
      !Array.isArray(pathRows) || pathRows.length !== 3 ||
      !Array.isArray(attributeRows) || attributeRows.length !==
        (Number(proof.schema_version) >= 10 ? 4 : 3) ||
      !Array.isArray(oldestExamples) || !Array.isArray(missingExamples)) {
      throw new Error("Inspiration proc formula-input snapshot coverage is incomplete");
    }
    const emittedPathEvents = new Map([
      ["critical_proc_bonus", Number(counts.emitted_critical_contributions)],
      ["lucky_proc_occurrence", Number(counts.emitted_lucky_contributions)],
      ["combined_lucky_occurrence_and_critical_bonus",
        Number(counts.emitted_combined_contributions)],
    ]);
    const expectedPathEvents = Number(proof.schema_version) >= 18
      ? new Map(pathRows.map((row) => [String(row?.path ?? ""), Number(row?.candidate_events)]))
      : emittedPathEvents;
    if (expectedPathEvents.size !== 3 ||
      [...expectedPathEvents.keys()].some((pathName) => !emittedPathEvents.has(pathName)) ||
      (Number(proof.schema_version) >= 18 &&
        (Number(expectedPathEvents.get("lucky_proc_occurrence")) !==
            Number(counts.emitted_lucky_contributions) ||
          Number(expectedPathEvents.get("critical_proc_bonus")) +
            Number(expectedPathEvents.get("combined_lucky_occurrence_and_critical_bonus")) !==
              blockedCriticalInterpretation ||
          [...expectedPathEvents.values()].reduce((sum, value) => sum + Number(value), 0) !==
            Number(counts.candidates_with_exact_window_magnitude)))) {
      throw new Error("Inspiration proc schema-18 candidate-path coverage is inconsistent");
    }
    const seenPaths = new Set();
    for (const row of pathRows) {
      const pathName = String(row?.path ?? "");
      const candidateEvents = Number(row?.candidate_events);
      const completeSets = Number(row?.complete_input_sets);
      const allWire = Number(row?.all_inputs_wire_provenance);
      const allNotAfter = Number(row?.all_inputs_observed_not_after_damage);
      if (!expectedPathEvents.has(pathName) || seenPaths.has(pathName) ||
        candidateEvents !== expectedPathEvents.get(pathName) ||
        ![candidateEvents, completeSets, allWire, allNotAfter].every(
          (value) => Number.isSafeInteger(value) && value >= 0,
        ) || completeSets > candidateEvents || allWire > completeSets ||
        allNotAfter > completeSets ||
        (allNotAfter > 0 &&
          (row.maximum_oldest_input_age_sequences == null ||
            row.maximum_oldest_input_age_micros == null ||
            !Number.isSafeInteger(Number(row.maximum_oldest_input_age_sequences)) ||
            Number(row.maximum_oldest_input_age_sequences) < 0 ||
            !Number.isSafeInteger(Number(row.maximum_oldest_input_age_micros)) ||
            Number(row.maximum_oldest_input_age_micros) < 0))) {
        throw new Error("Inspiration proc formula-input path coverage is inconsistent");
      }
      seenPaths.add(pathName);
      completeFormulaInputEvents += completeSets;
      allWireFormulaInputEvents += allWire;
      allNotAfterFormulaInputEvents += allNotAfter;
    }
    const expectedAttributeRequirements = new Map([
      [11710, Number(expectedPathEvents.get("critical_proc_bonus")) +
        Number(expectedPathEvents.get("combined_lucky_occurrence_and_critical_bonus"))],
      [11780, Number(expectedPathEvents.get("lucky_proc_occurrence")) +
        Number(expectedPathEvents.get("combined_lucky_occurrence_and_critical_bonus"))],
      [12510, Number(expectedPathEvents.get("critical_proc_bonus")) +
        Number(expectedPathEvents.get("combined_lucky_occurrence_and_critical_bonus"))],
      ...(Number(proof.schema_version) >= 10
        ? [[12530,
          Number(expectedPathEvents.get("combined_lucky_occurrence_and_critical_bonus"))]]
        : []),
    ]);
    const seenAttributes = new Set();
    for (const row of attributeRows) {
      const attributeId = Number(row?.attribute_id);
      const required = Number(row?.required_events);
      const present = Number(row?.present_events);
      const missing = Number(row?.missing_events);
      const wire = Number(row?.wire_provenance_events);
      const nonWire = Number(row?.non_wire_or_unknown_provenance_events);
      const notAfter = Number(row?.observed_not_after_damage_events);
      const after = Number(row?.observed_after_damage_events);
      const sameWire = Number(row?.same_wire_as_damage_events);
      if (!expectedAttributeRequirements.has(attributeId) || seenAttributes.has(attributeId) ||
        required !== expectedAttributeRequirements.get(attributeId) ||
        ![required, present, missing, wire, nonWire, notAfter, after, sameWire].every(
          (value) => Number.isSafeInteger(value) && value >= 0,
        ) || present + missing !== required || wire + nonWire !== present ||
        notAfter + after !== present || sameWire > wire ||
        (notAfter > 0 &&
          (row.maximum_age_sequences == null || row.maximum_age_micros == null ||
            !Number.isSafeInteger(Number(row.maximum_age_sequences)) ||
            Number(row.maximum_age_sequences) < 0 ||
            !Number.isSafeInteger(Number(row.maximum_age_micros)) ||
            Number(row.maximum_age_micros) < 0))) {
        throw new Error("Inspiration proc formula-input attribute coverage is inconsistent");
      }
      seenAttributes.add(attributeId);
      if (notAfter > 0) {
        maximumFormulaInputAgeMicros = Math.max(
          maximumFormulaInputAgeMicros ?? 0,
          Number(row.maximum_age_micros),
        );
      }
    }
    for (let index = 0; index < oldestExamples.length; index += 1) {
      const example = oldestExamples[index];
      if (!expectedPathEvents.has(String(example?.path ?? "")) ||
        !expectedAttributeRequirements.has(Number(example?.attribute_id)) ||
        !Number.isSafeInteger(Number(example?.damage_sequence)) ||
        !Number.isSafeInteger(Number(example?.attribute_sequence)) ||
        !Number.isSafeInteger(Number(example?.damage_observed_micros)) ||
        !Number.isSafeInteger(Number(example?.attribute_observed_micros)) ||
        Number(example.damage_sequence) - Number(example.attribute_sequence) !==
          Number(example.age_sequences) ||
        Number(example.damage_observed_micros) - Number(example.attribute_observed_micros) !==
          Number(example.age_micros) || Number(example.age_sequences) < 0 ||
        Number(example.age_micros) < 0 || typeof example?.wire_provenance !== "boolean" ||
        typeof example?.same_wire_as_damage !== "boolean" ||
        (example.same_wire_as_damage && !example.wire_provenance) ||
        (index > 0 && Number(oldestExamples[index - 1].age_micros) <
          Number(example.age_micros))) {
        throw new Error("Inspiration proc formula-input snapshot example is invalid");
      }
    }
    for (const example of missingExamples) {
      if (!expectedPathEvents.has(String(example?.path ?? "")) ||
        !expectedAttributeRequirements.has(Number(example?.attribute_id)) ||
        !Number.isSafeInteger(Number(example?.damage_sequence))) {
        throw new Error("Inspiration proc missing formula-input example is invalid");
      }
    }
    const missingInputs = attributeRows.reduce(
      (sum, row) => sum + Number(row.missing_events), 0,
    );
    if ((missingInputs === 0 && missingExamples.length !== 0) ||
      (missingInputs > 0 && missingExamples.length === 0)) {
      throw new Error("Inspiration proc missing formula-input examples do not match coverage");
    }
  }

  const integerStageCoverage = Number(proof.schema_version) >= 11
    ? proof.integer_stage_counterfactual_coverage
    : null;
  let integerStageCriticalEvents = 0;
  let integerStageCompatibleEvents = 0;
  let integerStageExactEvents = 0;
  let integerStageUnresolvedEvents = 0;
  let integerStageNoCompatibleEvents = 0;
  let damageSurfaceAudit = null;
  if (Number(proof.schema_version) >= 11) {
    const stageNumbers = [
      "exact_single_provider_candidate_events",
      "lucky_only_events_without_critical_stage",
      "critical_stage_events",
      "events_with_complete_stage_inputs",
      "events_without_complete_stage_inputs",
      "events_with_at_least_one_compatible_candidate",
      "events_without_compatible_candidates",
      "exact_stage_independent_events",
      "unresolved_stage_or_rounding_events",
    ];
    const expectedStageScope = Number(proof.schema_version) >= 17
      ? "exact_single_locally_observed_player_provider_window_with_exact_instance_magnitude; enumerate positive-integer latent bases for critical-only and critical-plus-lucky packet rows under additive-bonus and direct-total AttrCritDamage interpretations without requiring remote cast packets"
      : "exact_single_locally_observed_player_provider_window_with_exact_instance_magnitude; enumerate positive-integer latent bases for critical-only and critical-plus-lucky packet rows without requiring remote cast packets";
    const expectedCandidateFamilySize = Number(proof.schema_version) >= 17 ? 6 : 3;
    if (integerStageCoverage?.scope !== expectedStageScope ||
      !Array.isArray(integerStageCoverage?.candidate_family) ||
      integerStageCoverage.candidate_family.length !== expectedCandidateFamilySize ||
      integerStageCoverage.candidate_family.some((entry) => !String(entry)) ||
      integerStageCoverage?.candidate_family_authority !== false ||
      integerStageCoverage?.counterfactual_authority !== false ||
      stageNumbers.some((key) =>
        !Number.isSafeInteger(Number(integerStageCoverage?.[key])) ||
        Number(integerStageCoverage[key]) < 0) ||
      !Array.isArray(integerStageCoverage?.paths) || integerStageCoverage.paths.length !== 2 ||
      !Array.isArray(integerStageCoverage?.exact_examples) ||
      !Array.isArray(integerStageCoverage?.unresolved_examples)) {
      throw new Error("Inspiration proc integer-stage counterfactual coverage is incomplete");
    }
    const exactCandidates = Number(integerStageCoverage.exact_single_provider_candidate_events);
    const luckyOnly = Number(integerStageCoverage.lucky_only_events_without_critical_stage);
    integerStageCriticalEvents = Number(integerStageCoverage.critical_stage_events);
    const completeInputs = Number(integerStageCoverage.events_with_complete_stage_inputs);
    const missingInputs = Number(integerStageCoverage.events_without_complete_stage_inputs);
    integerStageCompatibleEvents = Number(
      integerStageCoverage.events_with_at_least_one_compatible_candidate,
    );
    integerStageNoCompatibleEvents = Number(
      integerStageCoverage.events_without_compatible_candidates,
    );
    integerStageExactEvents = Number(integerStageCoverage.exact_stage_independent_events);
    integerStageUnresolvedEvents = Number(
      integerStageCoverage.unresolved_stage_or_rounding_events,
    );
    if (exactCandidates !== (Number(proof.schema_version) >= 18
      ? Number(counts.candidates_with_exact_window_magnitude)
      : emitted) ||
      luckyOnly !== Number(counts.emitted_lucky_contributions) ||
      integerStageCriticalEvents !== (Number(proof.schema_version) >= 18
        ? blockedCriticalInterpretation
        : Number(counts.emitted_critical_contributions) +
          Number(counts.emitted_combined_contributions)) ||
      luckyOnly + integerStageCriticalEvents !== exactCandidates ||
      completeInputs + missingInputs !== integerStageCriticalEvents ||
      integerStageCompatibleEvents + integerStageNoCompatibleEvents !==
        integerStageCriticalEvents ||
      integerStageExactEvents + integerStageUnresolvedEvents !==
        integerStageCompatibleEvents) {
      throw new Error("Inspiration proc integer-stage totals do not conserve the exact subset");
    }
    const emittedStagePathEvents = new Map([
      ["critical_proc_bonus", Number(counts.emitted_critical_contributions)],
      ["combined_lucky_occurrence_and_critical_bonus",
        Number(counts.emitted_combined_contributions)],
    ]);
    const expectedStagePathEvents = Number(proof.schema_version) >= 18
      ? new Map(integerStageCoverage.paths.map(
        (row) => [String(row?.path ?? ""), Number(row?.events)],
      ))
      : emittedStagePathEvents;
    if (expectedStagePathEvents.size !== 2 ||
      [...expectedStagePathEvents.keys()].some((pathName) =>
        !emittedStagePathEvents.has(pathName)) ||
      [...expectedStagePathEvents.values()].reduce((sum, value) => sum + Number(value), 0) !==
        integerStageCriticalEvents) {
      throw new Error("Inspiration proc integer-stage path totals are inconsistent");
    }
    const seenStagePaths = new Set();
    for (const row of integerStageCoverage.paths) {
      const pathName = String(row?.path ?? "");
      const events = Number(row?.events);
      const complete = Number(row?.complete_stage_inputs);
      const compatible = Number(row?.compatible_candidate_events);
      const exact = Number(row?.exact_stage_independent_events);
      const unresolved = Number(row?.unresolved_stage_or_rounding_events);
      if (!expectedStagePathEvents.has(pathName) || seenStagePaths.has(pathName) ||
        events !== expectedStagePathEvents.get(pathName) ||
        ![events, complete, compatible, exact, unresolved].every(
          (value) => Number.isSafeInteger(value) && value >= 0,
        ) || complete > events || compatible > complete || exact + unresolved !== compatible) {
        throw new Error("Inspiration proc integer-stage path coverage is inconsistent");
      }
      seenStagePaths.add(pathName);
    }
    if (integerStageCoverage.paths.reduce((sum, row) => sum + Number(row.events), 0) !==
        integerStageCriticalEvents ||
      integerStageCoverage.paths.reduce(
        (sum, row) => sum + Number(row.complete_stage_inputs), 0,
      ) !== completeInputs ||
      integerStageCoverage.paths.reduce(
        (sum, row) => sum + Number(row.compatible_candidate_events), 0,
      ) !== integerStageCompatibleEvents ||
      integerStageCoverage.paths.reduce(
        (sum, row) => sum + Number(row.exact_stage_independent_events), 0,
      ) !== integerStageExactEvents ||
      integerStageCoverage.paths.reduce(
        (sum, row) => sum + Number(row.unresolved_stage_or_rounding_events), 0,
      ) !== integerStageUnresolvedEvents) {
      throw new Error("Inspiration proc integer-stage path sums are inconsistent");
    }
    if (Number(proof.schema_version) >= 17) {
      const interpretationRows = integerStageCoverage?.critical_factor_interpretation_breakdown;
      const validRelations = new Map([
        ["both", new Set(["same_exact", "divergent_exact", "within_interpretation_unresolved"])],
        ["additive_only", new Set(["single_interpretation_exact", "within_interpretation_unresolved"])],
        ["direct_only", new Set(["single_interpretation_exact", "within_interpretation_unresolved"])],
        ["neither", new Set(["no_compatible_interpretation"])],
      ]);
      const seenInterpretationRows = new Set();
      if (integerStageCoverage?.critical_factor_interpretation_breakdown_authority !== false ||
        !Array.isArray(interpretationRows) || interpretationRows.length === 0) {
        throw new Error("Inspiration proc critical-factor interpretation breakdown is missing");
      }
      for (const row of interpretationRows) {
        const pathName = String(row?.path ?? "");
        const compatibility = String(row?.compatibility ?? "");
        const relation = String(row?.counterfactual_relation ?? "");
        const key = stableStringify([pathName, compatibility, relation]);
        if (!expectedStagePathEvents.has(pathName) ||
          !validRelations.get(compatibility)?.has(relation) ||
          seenInterpretationRows.has(key) || row?.formula_authority !== false ||
          !Number.isSafeInteger(Number(row?.events)) || Number(row.events) <= 0) {
          throw new Error("Inspiration proc critical-factor interpretation row is invalid");
        }
        seenInterpretationRows.add(key);
      }
      if (interpretationRows.reduce((sum, row) => sum + Number(row.events), 0) !==
          integerStageCriticalEvents) {
        throw new Error("Inspiration proc critical-factor interpretation rows do not conserve events");
      }
    }
    const validateStageExample = (example, exactExpected) => {
      const pathName = String(example?.path ?? "");
      const combined = pathName === "combined_lucky_occurrence_and_critical_bonus";
      if (!expectedStagePathEvents.has(pathName) ||
        !Number.isSafeInteger(Number(example?.damage_sequence)) ||
        !Number.isSafeInteger(Number(example?.observed_damage)) ||
        Number(example.observed_damage) <= 0 ||
        !Number.isSafeInteger(Number(example?.critical_damage_raw)) ||
        Number(example.critical_damage_raw) <= 0 ||
        (combined && (!Number.isSafeInteger(Number(example?.lucky_damage_raw)) ||
          Number(example.lucky_damage_raw) <= 0)) ||
        (!combined && example?.lucky_damage_raw != null) ||
        !Array.isArray(example?.candidates) ||
        example.candidates.length !== (Number(proof.schema_version) >= 17
          ? (combined ? 20 : 4)
          : (combined ? 10 : 2))) {
        throw new Error("Inspiration proc integer-stage example identity is invalid");
      }
      const compatible = [];
      for (const candidate of example.candidates) {
        const isCompatible = candidate?.compatible_with_observed_damage === true;
        if (!["floor", "nearest_half_up"].includes(candidate?.first_rounding) ||
          (Number(proof.schema_version) >= 17 &&
            !["additive_bonus", "direct_total"].includes(
              candidate?.critical_factor_interpretation,
            )) ||
          (candidate?.second_rounding != null &&
            !["floor", "nearest_half_up"].includes(candidate.second_rounding)) ||
          typeof candidate?.evaluation_status !== "string" ||
          typeof candidate?.compatible_with_observed_damage !== "boolean") {
          throw new Error("Inspiration proc integer-stage candidate metadata is invalid");
        }
        const ranges = [candidate?.latent_base_min, candidate?.latent_base_max,
          candidate?.counterfactual_min, candidate?.counterfactual_max];
        if (isCompatible) {
          if (candidate.evaluation_status !== "compatible_integer_preimage" ||
            !ranges.every((value) => Number.isSafeInteger(Number(value)) && Number(value) >= 0) ||
            Number(candidate.latent_base_min) > Number(candidate.latent_base_max) ||
            Number(candidate.counterfactual_min) > Number(candidate.counterfactual_max)) {
            throw new Error("Inspiration proc compatible integer-stage candidate is invalid");
          }
          compatible.push(candidate);
        } else if (ranges.some((value) => value != null)) {
          throw new Error("Inspiration proc incompatible stage candidate carries invented ranges");
        }
      }
      const converged = compatible.length > 0 &&
        compatible.every((candidate) =>
          Number(candidate.counterfactual_min) === Number(candidate.counterfactual_max)) &&
        compatible.every((candidate) =>
          Number(candidate.counterfactual_min) ===
            Number(compatible[0].counterfactual_min));
      if (converged !== exactExpected ||
        (exactExpected &&
          (Number(example?.exact_noncritical_counterfactual) !==
            Number(compatible[0].counterfactual_min) ||
            Number(example?.exact_critical_bonus) !== Number(example.observed_damage) -
              Number(example.exact_noncritical_counterfactual))) ||
        (!exactExpected && (example?.exact_noncritical_counterfactual != null ||
          example?.exact_critical_bonus != null))) {
        throw new Error("Inspiration proc integer-stage example convergence is inconsistent");
      }
    };
    for (const example of integerStageCoverage.exact_examples) {
      validateStageExample(example, true);
    }
    for (const example of integerStageCoverage.unresolved_examples) {
      validateStageExample(example, false);
    }
    if (Number(proof.schema_version) >= 12) {
      const surfaceSchema = Number(proof.damage_formula_surface_schema_version);
      if (![1, 2].includes(surfaceSchema)) {
        throw new Error("Inspiration proc damage formula surface schema is unsupported");
      }
      damageSurfaceAudit = validateInspirationDamageSurfaceJoin(
        integerStageCoverage.damage_surface_join,
        {
          criticalEvents: integerStageCriticalEvents,
          compatibleEvents: integerStageCompatibleEvents,
          exactEvents: integerStageExactEvents,
          unresolvedEvents: integerStageUnresolvedEvents,
          noCompatibleEvents: integerStageNoCompatibleEvents,
          exactStageInputIdentity: Number(proof.schema_version) >= 15,
          exactStageInputFreshness: Number(proof.schema_version) >= 16,
        },
      );
    }
  }

  let closureIntegerStageCoverage = Number(proof.schema_version) >= 11
    ? integerStageCoverage
    : null;
  if (Number(proof.schema_version) >= 18) {
    const perEventRecords = integerStageCoverage?.critical_factor_event_records;
    if (!Array.isArray(perEventRecords) || perEventRecords.length !== integerStageCriticalEvents) {
      throw new Error("Inspiration schema-18 per-event critical-factor evidence is incomplete");
    }
    const { critical_factor_event_records: _omittedPerEventRecords, ...compactCoverage } =
      integerStageCoverage;
    closureIntegerStageCoverage = structuredClone(compactCoverage);
    closureIntegerStageCoverage.critical_factor_event_records_retained_in_bound_proof =
      perEventRecords.length;
    closureIntegerStageCoverage.critical_factor_event_records_inlined_in_closure = false;
  } else if (closureIntegerStageCoverage) {
    closureIntegerStageCoverage = structuredClone(closureIntegerStageCoverage);
  }

  return {
    effect_id: "2202041",
    ...(Number(proof.schema_version) >= 6 ? {
      parent_effect_id: "2202040",
      exact_origin_source_type_id: 1,
      exact_origin_source_config_id: "2202040",
    } : {}),
    mechanic: "critical-and-lucky-proc-chance",
    proof_state: Number(proof.schema_version) >= 17
      ? "current-build-multi-session-locally-observed-parent-origin-level-lifecycle-formula-input-integer-stage-exact-build-damage-surface-and-critical-factor-interpretation-audited-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 12
      ? "current-build-multi-session-locally-observed-parent-origin-level-lifecycle-formula-input-integer-stage-and-exact-build-damage-surface-audited-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 11
      ? "current-build-multi-session-locally-observed-parent-origin-level-lifecycle-formula-input-and-integer-stage-counterfactual-audited-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 10
      ? "current-build-multi-session-locally-observed-parent-origin-level-lifecycle-and-combined-stage-input-snapshot-audited-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 9
      ? "current-build-multi-session-locally-observed-parent-origin-level-lifecycle-and-formula-input-snapshot-audited-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 8
        ? "current-build-multi-session-locally-observed-parent-origin-level-and-lifecycle-example-audited-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 7
        ? "current-build-multi-session-locally-observed-parent-origin-level-and-lifecycle-audited-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 6
        ? "current-build-multi-session-locally-observed-parent-origin-and-level-bound-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 5
        ? "current-build-multi-session-locally-observed-level-bound-single-provider-subset-arithmetic-conserved-formula-open"
      : Number(proof.schema_version) >= 4
        ? "current-build-multi-session-locally-observed-single-provider-subset-arithmetic-conserved-formula-open"
        : "current-build-locally-observed-single-provider-subset-arithmetic-conserved-formula-open",
    proof: fileDescriptor(proofPath),
    proof_schema_version: Number(proof.schema_version),
    exact_rlogs: rlogs.length,
    exact_stat_transform_proven: false,
    exact_instance_magnitudes_proven: true,
    proven_instance_windows: magnitudes.length,
    independent_sessions_with_magnitudes: magnitudeSessions.size,
    magnitude_raw_deltas: magnitudeRawDeltas,
    ...(Number(proof.schema_version) >= 5 ? { magnitude_by_level: magnitudeByLevel } : {}),
    ...(Number(proof.schema_version) >= 7 ? {
      level_lifecycle_evidence: structuredClone(levelLifecycleEvidence),
      reversible_static_levels_proven: reversibleStaticLevelsProven,
      all_observed_levels_reversible_static_transform_proven:
        reversibleStaticLevelsProven === levelLifecycleEvidence.length,
    } : {}),
    ...(Number(proof.schema_version) >= 9 ? {
      formula_input_snapshot_coverage: structuredClone(formulaInputSnapshotCoverage),
      formula_input_events_with_complete_sets: completeFormulaInputEvents,
      formula_input_events_all_wire_provenance: allWireFormulaInputEvents,
      formula_input_events_observed_not_after_damage: allNotAfterFormulaInputEvents,
      maximum_formula_input_age_micros: maximumFormulaInputAgeMicros,
      event_time_formula_input_snapshot_proven: false,
    } : {}),
    ...(Number(proof.schema_version) >= 11 ? {
      integer_stage_counterfactual_coverage: closureIntegerStageCoverage,
      integer_stage_critical_events: integerStageCriticalEvents,
      integer_stage_events_with_compatible_candidates: integerStageCompatibleEvents,
      integer_stage_exact_counterfactual_events: integerStageExactEvents,
      integer_stage_unresolved_order_or_rounding_events: integerStageUnresolvedEvents,
      integer_stage_events_without_compatible_candidates: integerStageNoCompatibleEvents,
      integer_stage_candidate_family_proven: false,
      integer_stage_counterfactual_proven: false,
      ...(Number(proof.schema_version) >= 17 ? {
        critical_factor_interpretation_breakdown:
          structuredClone(integerStageCoverage.critical_factor_interpretation_breakdown),
        critical_factor_interpretation_breakdown_authority: false,
        critical_damage_raw_interpretation_authority: false,
      } : {}),
    } : {}),
    ...(Number(proof.schema_version) >= 12 ? {
      damage_formula_surface: structuredClone(proof.damage_formula_surface),
      damage_formula_surface_schema_version: Number(proof.damage_formula_surface_schema_version),
      damage_surface_join: structuredClone(integerStageCoverage.damage_surface_join),
      damage_surface_identity_groups: damageSurfaceAudit.identityGroups,
      damage_surface_events_with_unique_row: damageSurfaceAudit.uniqueRowEvents,
      damage_surface_events_with_ambiguous_rows: damageSurfaceAudit.ambiguousRowEvents,
      damage_surface_events_without_row: damageSurfaceAudit.missingRowEvents,
      damage_surface_events_with_resolved_script: damageSurfaceAudit.resolvedScriptEvents,
      damage_surface_events_without_resolved_script: damageSurfaceAudit.unresolvedScriptEvents,
      ...(Number(proof.schema_version) >= 13 ? {
        damage_surface_events_with_unique_ability_candidate_when_hit_event_absent:
          damageSurfaceAudit.uniqueAbilityDiagnosticEvents,
        damage_surface_events_with_unique_ability_candidate_and_resolved_script_when_hit_event_absent:
          damageSurfaceAudit.uniqueAbilityResolvedScriptDiagnosticEvents,
        damage_surface_events_without_exact_or_unique_ability_candidate:
          damageSurfaceAudit.remainingWithoutExactOrUniqueEvents,
        unique_ability_damage_surface_candidate_authority: false,
        missing_hit_event_id_synthesized: false,
      } : {}),
      ...(Number(proof.schema_version) >= 14 ? {
        damage_script_preimage_breakdown:
          structuredClone(integerStageCoverage.damage_surface_join.damage_script_preimage_breakdown),
        damage_script_preimage_breakdown_authority: false,
      } : {}),
      ...(Number(proof.schema_version) >= 15 ? {
        damage_surface_identity_groups_include_exact_stage_inputs: true,
      } : {}),
      ...(Number(proof.schema_version) >= 16 ? {
        stage_input_freshness_breakdown:
          structuredClone(integerStageCoverage.damage_surface_join.stage_input_freshness_breakdown),
        stage_input_freshness_breakdown_authority: false,
        damage_surface_identity_groups_include_exact_stage_input_freshness: true,
      } : {}),
      damage_formula_surface_runtime_authority: false,
      damage_script_identity_proves_operator: false,
      owner_stage_array_selection_proves_formula: false,
    } : {}),
    exact_packet_duration_rows: Number(counts.status_activation_rows_with_exact_packet_duration),
    exact_single_provider_damage_events: Number(proof.schema_version) >= 18
      ? Number(counts.candidates_with_exact_window_magnitude)
      : emitted,
    ...(Number(proof.schema_version) >= 18 ? {
      exact_rational_contribution_events: emitted,
      critical_interpretation_blocked_candidate_events: blockedCriticalInterpretation,
    } : {}),
    critical_only_diagnostic_events: Number(counts.emitted_critical_contributions),
    lucky_only_diagnostic_events: Number(counts.emitted_lucky_contributions),
    combined_diagnostic_events: Number(counts.emitted_combined_contributions),
    exact_rational_bucket_arithmetic_conserved: true,
    opportunity_counterfactual_proven: false,
    observed_damage_reassigned_to_provider: 0,
    blockers: [
      "per-level reversible static transform gates remain open; see level_lifecycle_evidence",
      "unobserved effect levels remain unresolved and are never interpolated",
      "proc expected-value attribution formula operation order and rounding lack current-build authority",
      ...(Number(proof.schema_version) >= 11 ? [
        `${integerStageNoCompatibleEvents} critical-stage packet rows have no integer preimage under the audited candidate family, proving additional or different stages remain`,
        `${integerStageUnresolvedEvents} compatible packet rows retain stage-order or rounding disagreement`,
        "the enumerated integer-stage candidate family lacks exact-build formula-family authority",
      ] : []),
      ...(Number(proof.schema_version) >= 12 ? [
        `${damageSurfaceAudit.missingRowEvents} critical-stage packet rows have no exact-build damage-surface candidate`,
        `${damageSurfaceAudit.ambiguousRowEvents} critical-stage packet rows retain ambiguous exact-build damage-surface candidates`,
        `${damageSurfaceAudit.unresolvedScriptEvents} critical-stage packet rows have no uniquely resolved DamageScript`,
        "exact-build DamageScript identity and coefficient arrays are grouping evidence, not server operation-order or rounding authority",
        "owner-stage coefficient-array selection is diagnostic and has no formula authority",
      ] : []),
      ...(Number(proof.schema_version) >= 13 ? [
        `${damageSurfaceAudit.uniqueAbilityDiagnosticEvents} packet rows with absent hit-event ID have one unique exact-build ability-level surface candidate for grouping only`,
        `${damageSurfaceAudit.remainingWithoutExactOrUniqueEvents} packet rows retain no exact or unique ability-level surface candidate`,
        "ability-only surface candidates never synthesize a missing hit-event ID and have no formula authority",
      ] : []),
      ...(Number(proof.schema_version) >= 14 ? [
        "DamageScript preimage breakdown is a conserved diagnostic partition and has no formula authority",
      ] : []),
      ...(Number(proof.schema_version) >= 16 ? [
        "stage-input freshness partitions are exact local-observation diagnostics and have no event-time formula authority",
      ] : []),
      ...(Number(proof.schema_version) >= 17 ? [
        "AttrCritDamage additive-bonus versus direct-total interpretation remains unresolved and neither candidate has formula authority",
      ] : []),
      "last-observed formula inputs have no event-time freshness authority; see formula_input_snapshot_coverage",
      "full Inspiration effect components are not composed and conserved",
      "canonical replay conservation and protocol-event coverage remain required",
    ],
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function inspirationStageInputAgeBucket(observationState, oldestAgeMicros) {
  if (observationState !== "complete_not_after_damage") {
    if (observationState === "missing_required_stage_input") return "missing";
    if (observationState === "observed_after_damage") return "observed_after_damage";
    return "invalid_observation_state";
  }
  if (oldestAgeMicros === 0) return "same_observed_micros";
  if (oldestAgeMicros >= 1 && oldestAgeMicros <= 100_000) return "1us_to_100ms";
  if (oldestAgeMicros <= 500_000) return "100ms_to_500ms";
  if (oldestAgeMicros <= 1_000_000) return "500ms_to_1s";
  if (oldestAgeMicros <= 5_000_000) return "1s_to_5s";
  if (oldestAgeMicros > 5_000_000) return "over_5s";
  return "missing_age";
}

function validateInspirationDamageSurfaceJoin(join, expected) {
  const exactStageInputIdentity = expected?.exactStageInputIdentity === true;
  const exactStageInputFreshness = expected?.exactStageInputFreshness === true;
  const hasUniqueAbilityDiagnostic = Object.prototype.hasOwnProperty.call(
    join ?? {},
    "events_with_unique_ability_surface_candidate_when_hit_event_absent",
  );
  const hasDamageScriptPreimageBreakdown = Object.prototype.hasOwnProperty.call(
    join ?? {},
    "damage_script_preimage_breakdown",
  );
  const hasStageInputFreshnessBreakdown = Object.prototype.hasOwnProperty.call(
    join ?? {},
    "stage_input_freshness_breakdown",
  );
  const countFields = [
    "identity_groups",
    "events",
    "events_with_exactly_one_surface_row",
    "events_with_ambiguous_surface_rows",
    "events_without_surface_row",
    "events_with_resolved_damage_script",
    "events_without_resolved_damage_script",
    ...(hasUniqueAbilityDiagnostic ? [
      "events_with_unique_ability_surface_candidate_when_hit_event_absent",
      "events_with_unique_ability_surface_candidate_and_resolved_damage_script_when_hit_event_absent",
      "events_without_exact_or_unique_ability_surface_candidate",
    ] : []),
  ];
  if (join?.surface_runtime_formula_authority !== false ||
    (hasUniqueAbilityDiagnostic && join?.unique_ability_surface_candidate_authority !== false) ||
    (hasDamageScriptPreimageBreakdown &&
      join?.damage_script_preimage_breakdown_authority !== false) ||
    (exactStageInputFreshness && !hasStageInputFreshnessBreakdown) ||
    (hasStageInputFreshnessBreakdown &&
      join?.stage_input_freshness_breakdown_authority !== false) ||
    countFields.some((field) =>
      !Number.isSafeInteger(Number(join?.[field])) || Number(join[field]) < 0) ||
    !Array.isArray(join?.groups) || Number(join.identity_groups) !== join.groups.length ||
    (hasDamageScriptPreimageBreakdown &&
      !Array.isArray(join.damage_script_preimage_breakdown)) ||
    (hasStageInputFreshnessBreakdown &&
      !Array.isArray(join.stage_input_freshness_breakdown)) ||
    Number(join.events) !== expected.criticalEvents ||
    Number(join.events_with_exactly_one_surface_row) +
      Number(join.events_with_ambiguous_surface_rows) +
      Number(join.events_without_surface_row) !== Number(join.events) ||
    Number(join.events_with_resolved_damage_script) +
      Number(join.events_without_resolved_damage_script) !== Number(join.events)) {
    throw new Error("Inspiration proc damage-surface join summary is inconsistent");
  }

  const optionalIntegerFields = [
    "ability_id", "hit_event_id", "damage_source", "damage_type", "type_flags",
    "owner_level", "owner_stage", "property", "passive_uuid", "damage_mode",
    "skill_effect_uuid", "skill_effect_group_index", "skill_effect_component_index",
    "skill_effect_component_count",
  ];
  const optionalBooleanFields = ["reported_critical", "normal_hit", "rainbow"];
  const groupCountFields = [
    "events", "complete_stage_inputs", "compatible_candidate_events",
    "exact_stage_independent_events", "unresolved_stage_or_rounding_events",
    "events_without_compatible_candidates",
  ];
  const resolutions = new Map([
    ["no_exact_build_surface_row", 0],
    ["exactly_one_exact_build_surface_row", 1],
    ["ambiguous_exact_build_surface_rows", 2],
  ]);
  const identities = new Set();
  const expectedDamageScriptBreakdown = new Map();
  const expectedStageInputFreshnessBreakdown = new Map();
  const totals = {
    events: 0,
    complete: 0,
    compatible: 0,
    exact: 0,
    unresolved: 0,
    noCompatible: 0,
    unique: 0,
    ambiguous: 0,
    missing: 0,
    resolvedScript: 0,
    unresolvedScript: 0,
    uniqueAbilityDiagnostic: 0,
    uniqueAbilityResolvedScript: 0,
    remainingWithoutExactOrUnique: 0,
  };
  for (const group of join.groups) {
    const pathName = String(group?.path ?? "");
    const stageInputObservationState = String(group?.stage_input_observation_state ?? "");
    const oldestStageInputAgeSequences = group?.oldest_stage_input_age_sequences;
    const oldestStageInputAgeMicros = group?.oldest_stage_input_age_micros;
    const completeFreshness = stageInputObservationState === "complete_not_after_damage";
    if (!["critical_proc_bonus", "combined_lucky_occurrence_and_critical_bonus"].includes(pathName) ||
      optionalIntegerFields.some((field) => group?.[field] != null &&
        !Number.isSafeInteger(Number(group[field]))) ||
      optionalBooleanFields.some((field) => group?.[field] != null &&
        typeof group[field] !== "boolean") ||
      groupCountFields.some((field) =>
        !Number.isSafeInteger(Number(group?.[field])) || Number(group[field]) < 0) ||
      Number(group.events) <= 0 || Number(group.complete_stage_inputs) > Number(group.events) ||
      Number(group.compatible_candidate_events) +
        Number(group.events_without_compatible_candidates) !==
        Number(group.complete_stage_inputs) ||
      Number(group.exact_stage_independent_events) +
        Number(group.unresolved_stage_or_rounding_events) !==
        Number(group.compatible_candidate_events) ||
      !Array.isArray(group?.critical_damage_raw_values) ||
      !Array.isArray(group?.lucky_damage_raw_values) ||
      (exactStageInputIdentity &&
        (group.critical_damage_raw_values.length > 1 ||
          group.lucky_damage_raw_values.length > 1)) ||
      group.critical_damage_raw_values.some((value) =>
        !Number.isSafeInteger(Number(value)) || Number(value) <= 0) ||
      group.lucky_damage_raw_values.some((value) =>
        !Number.isSafeInteger(Number(value)) || Number(value) <= 0) ||
      (exactStageInputFreshness &&
        (![
          "complete_not_after_damage",
          "missing_required_stage_input",
          "observed_after_damage",
        ].includes(stageInputObservationState) ||
          typeof group?.stage_inputs_all_wire_provenance !== "boolean" ||
          typeof group?.stage_inputs_all_same_wire_as_damage !== "boolean" ||
          (completeFreshness &&
            (!Number.isSafeInteger(Number(oldestStageInputAgeSequences)) ||
              Number(oldestStageInputAgeSequences) < 0 ||
              !Number.isSafeInteger(Number(oldestStageInputAgeMicros)) ||
              Number(oldestStageInputAgeMicros) < 0)) ||
          (!completeFreshness &&
            (oldestStageInputAgeSequences != null || oldestStageInputAgeMicros != null)))) ||
      !/^\d+$/.test(String(group?.observed_damage_sum ?? "")) ||
      !Array.isArray(group?.damage_surface_candidates) ||
      (hasUniqueAbilityDiagnostic &&
        !Array.isArray(group?.unique_ability_damage_surface_candidates))) {
      throw new Error("Inspiration proc damage-surface identity group is invalid");
    }
    const identity = stableStringify([
      pathName,
      ...optionalIntegerFields.map((field) => group[field] ?? null),
      ...optionalBooleanFields.map((field) => group[field] ?? null),
      ...(exactStageInputIdentity ? [
        group.critical_damage_raw_values[0] ?? null,
        group.lucky_damage_raw_values[0] ?? null,
      ] : []),
      ...(exactStageInputFreshness ? [
        stageInputObservationState,
        oldestStageInputAgeSequences ?? null,
        oldestStageInputAgeMicros ?? null,
        group.stage_inputs_all_wire_provenance,
        group.stage_inputs_all_same_wire_as_damage,
      ] : []),
    ]);
    if (identities.has(identity)) {
      throw new Error("Inspiration proc damage-surface identity group is duplicated");
    }
    identities.add(identity);
    try {
      if (BigInt(group.observed_damage_sum) <= 0n) {
        throw new Error("non-positive sum");
      }
    } catch {
      throw new Error("Inspiration proc damage-surface observed damage sum is invalid");
    }

    const resolution = String(group.damage_surface_resolution ?? "");
    const candidateClass = resolutions.get(resolution);
    if (candidateClass === undefined ||
      (candidateClass < 2 && group.damage_surface_candidates.length !== candidateClass) ||
      (candidateClass === 2 && group.damage_surface_candidates.length < 2)) {
      throw new Error("Inspiration proc damage-surface resolution does not match candidates");
    }
    const uniqueAbilityCandidates = hasUniqueAbilityDiagnostic
      ? group.unique_ability_damage_surface_candidates
      : [];
    if (hasUniqueAbilityDiagnostic) {
      const uniqueResolution = String(group.unique_ability_damage_surface_resolution ?? "");
      const exactRowsPresent = candidateClass > 0;
      const hitEventPresent = group.hit_event_id != null;
      const validUniqueResolution =
        (uniqueResolution === "not_applicable_exact_hit_event_surface_present" &&
          exactRowsPresent && uniqueAbilityCandidates.length === 0) ||
        (uniqueResolution === "hit_event_present_no_exact_surface_row" &&
          !exactRowsPresent && hitEventPresent && uniqueAbilityCandidates.length === 0) ||
        (uniqueResolution === "hit_event_absent_no_ability_surface_candidate" &&
          !exactRowsPresent && !hitEventPresent && uniqueAbilityCandidates.length === 0) ||
        (uniqueResolution ===
            "hit_event_absent_one_unique_ability_surface_candidate_diagnostic" &&
          !exactRowsPresent && !hitEventPresent && uniqueAbilityCandidates.length === 1) ||
        (uniqueResolution === "hit_event_absent_ambiguous_ability_surface_candidates" &&
          !exactRowsPresent && !hitEventPresent && uniqueAbilityCandidates.length >= 2);
      if (!validUniqueResolution) {
        throw new Error("Inspiration proc unique-ability surface diagnostic is inconsistent");
      }
    }
    for (const candidate of [...group.damage_surface_candidates, ...uniqueAbilityCandidates]) {
      const ratio = candidate?.pve_damage_ratio;
      const fixed = candidate?.pve_fixed_parameter;
      if (!/^\d+$/.test(String(candidate?.damage_id ?? "")) ||
        (candidate?.damage_script != null &&
          (typeof candidate.damage_script !== "string" || candidate.damage_script.length === 0)) ||
        !Array.isArray(ratio) || !Array.isArray(fixed) ||
        [...ratio, ...fixed].some((value) => !Number.isSafeInteger(Number(value))) ||
        candidate?.owner_stage_selection_authority !== false) {
        throw new Error("Inspiration proc damage-surface candidate is invalid");
      }
      const selectedValue = (values) => {
        if (values.length === 0) return null;
        if (values.length === 1) return Number(values[0]);
        const ownerStage = group.owner_stage;
        return ownerStage != null && Number.isSafeInteger(Number(ownerStage)) &&
          Number(ownerStage) >= 0 &&
          Number(ownerStage) < values.length
          ? Number(values[Number(ownerStage)])
          : null;
      };
      if ((candidate.selected_pve_damage_ratio == null
        ? null : Number(candidate.selected_pve_damage_ratio)) !== selectedValue(ratio) ||
        (candidate.selected_pve_fixed_parameter == null
          ? null : Number(candidate.selected_pve_fixed_parameter)) !== selectedValue(fixed)) {
        throw new Error("Inspiration proc owner-stage coefficient selection is inconsistent");
      }
    }

    const events = Number(group.events);
    totals.events += events;
    totals.complete += Number(group.complete_stage_inputs);
    totals.compatible += Number(group.compatible_candidate_events);
    totals.exact += Number(group.exact_stage_independent_events);
    totals.unresolved += Number(group.unresolved_stage_or_rounding_events);
    totals.noCompatible += Number(group.events_without_compatible_candidates);
    if (candidateClass === 0) totals.missing += events;
    else if (candidateClass === 1) totals.unique += events;
    else totals.ambiguous += events;
    const resolvedScript = candidateClass === 1 &&
      typeof group.damage_surface_candidates[0]?.damage_script === "string";
    if (resolvedScript) totals.resolvedScript += events;
    else totals.unresolvedScript += events;
    if (hasUniqueAbilityDiagnostic && candidateClass === 0) {
      if (uniqueAbilityCandidates.length === 1) {
        totals.uniqueAbilityDiagnostic += events;
        if (typeof uniqueAbilityCandidates[0]?.damage_script === "string") {
          totals.uniqueAbilityResolvedScript += events;
        }
      } else {
        totals.remainingWithoutExactOrUnique += events;
      }
    }
    if (hasDamageScriptPreimageBreakdown) {
      let surfaceBinding;
      let damageScript;
      if (candidateClass === 1) {
        surfaceBinding = "exact_hit_event";
        damageScript = typeof group.damage_surface_candidates[0]?.damage_script === "string"
          ? group.damage_surface_candidates[0].damage_script
          : "<missing_damage_script>";
      } else if (candidateClass === 2) {
        surfaceBinding = "ambiguous_exact_hit_event";
        damageScript = "<ambiguous_damage_script>";
      } else if (uniqueAbilityCandidates.length === 1) {
        surfaceBinding = "ability_only_diagnostic";
        damageScript = typeof uniqueAbilityCandidates[0]?.damage_script === "string"
          ? uniqueAbilityCandidates[0].damage_script
          : "<missing_damage_script>";
      } else {
        surfaceBinding = "unresolved";
        damageScript = uniqueAbilityCandidates.length > 1
          ? "<ambiguous_damage_script>"
          : "<missing_damage_script>";
      }
      const breakdownKey = stableStringify([surfaceBinding, damageScript]);
      const breakdown = expectedDamageScriptBreakdown.get(breakdownKey) ?? {
        surface_binding: surfaceBinding,
        damage_script: damageScript,
        identity_groups: 0,
        events: 0,
        complete_stage_inputs: 0,
        compatible_candidate_events: 0,
        exact_stage_independent_events: 0,
        unresolved_stage_or_rounding_events: 0,
        events_without_compatible_candidates: 0,
      };
      breakdown.identity_groups += 1;
      breakdown.events += events;
      breakdown.complete_stage_inputs += Number(group.complete_stage_inputs);
      breakdown.compatible_candidate_events += Number(group.compatible_candidate_events);
      breakdown.exact_stage_independent_events += Number(group.exact_stage_independent_events);
      breakdown.unresolved_stage_or_rounding_events +=
        Number(group.unresolved_stage_or_rounding_events);
      breakdown.events_without_compatible_candidates +=
        Number(group.events_without_compatible_candidates);
      expectedDamageScriptBreakdown.set(breakdownKey, breakdown);
    }
    if (hasStageInputFreshnessBreakdown) {
      const oldestAgeMicros = oldestStageInputAgeMicros == null
        ? null
        : Number(oldestStageInputAgeMicros);
      const oldestAgeBucket = inspirationStageInputAgeBucket(
        stageInputObservationState,
        oldestAgeMicros,
      );
      const allWireProvenance = group.stage_inputs_all_wire_provenance === true;
      const allSameWireAsDamage = group.stage_inputs_all_same_wire_as_damage === true;
      const freshnessKey = stableStringify([
        pathName,
        stageInputObservationState,
        oldestAgeBucket,
        allWireProvenance,
        allSameWireAsDamage,
      ]);
      const freshness = expectedStageInputFreshnessBreakdown.get(freshnessKey) ?? {
        path: pathName,
        observation_state: stageInputObservationState,
        oldest_age_bucket: oldestAgeBucket,
        all_wire_provenance: allWireProvenance,
        all_same_wire_as_damage: allSameWireAsDamage,
        identity_groups: 0,
        events: 0,
        complete_stage_inputs: 0,
        compatible_candidate_events: 0,
        exact_stage_independent_events: 0,
        unresolved_stage_or_rounding_events: 0,
        events_without_compatible_candidates: 0,
      };
      freshness.identity_groups += 1;
      freshness.events += events;
      freshness.complete_stage_inputs += Number(group.complete_stage_inputs);
      freshness.compatible_candidate_events += Number(group.compatible_candidate_events);
      freshness.exact_stage_independent_events += Number(group.exact_stage_independent_events);
      freshness.unresolved_stage_or_rounding_events +=
        Number(group.unresolved_stage_or_rounding_events);
      freshness.events_without_compatible_candidates +=
        Number(group.events_without_compatible_candidates);
      expectedStageInputFreshnessBreakdown.set(freshnessKey, freshness);
    }
  }
  if (totals.events !== Number(join.events) ||
    totals.compatible !== expected.compatibleEvents || totals.exact !== expected.exactEvents ||
    totals.unresolved !== expected.unresolvedEvents ||
    totals.noCompatible !== expected.noCompatibleEvents ||
    totals.unique !== Number(join.events_with_exactly_one_surface_row) ||
    totals.ambiguous !== Number(join.events_with_ambiguous_surface_rows) ||
    totals.missing !== Number(join.events_without_surface_row) ||
    totals.resolvedScript !== Number(join.events_with_resolved_damage_script) ||
    totals.unresolvedScript !== Number(join.events_without_resolved_damage_script) ||
    (hasUniqueAbilityDiagnostic &&
      (totals.uniqueAbilityDiagnostic !==
          Number(join.events_with_unique_ability_surface_candidate_when_hit_event_absent) ||
        totals.uniqueAbilityResolvedScript !== Number(
          join.events_with_unique_ability_surface_candidate_and_resolved_damage_script_when_hit_event_absent,
        ) ||
        totals.remainingWithoutExactOrUnique !==
          Number(join.events_without_exact_or_unique_ability_surface_candidate) ||
        totals.uniqueAbilityDiagnostic + totals.remainingWithoutExactOrUnique !== totals.missing))) {
    throw new Error("Inspiration proc damage-surface group totals do not conserve the audit");
  }
  if (hasDamageScriptPreimageBreakdown) {
    const breakdownCountFields = [
      "identity_groups", "events", "complete_stage_inputs", "compatible_candidate_events",
      "exact_stage_independent_events", "unresolved_stage_or_rounding_events",
      "events_without_compatible_candidates",
    ];
    const seenBreakdowns = new Set();
    for (const row of join.damage_script_preimage_breakdown) {
      const key = stableStringify([String(row?.surface_binding ?? ""), String(row?.damage_script ?? "")]);
      const expectedRow = expectedDamageScriptBreakdown.get(key);
      if (!expectedRow || seenBreakdowns.has(key) || row?.formula_authority !== false ||
        !String(row?.surface_binding ?? "") || !String(row?.damage_script ?? "") ||
        breakdownCountFields.some((field) =>
          !Number.isSafeInteger(Number(row?.[field])) || Number(row[field]) < 0 ||
          Number(row[field]) !== Number(expectedRow[field]))) {
        throw new Error("Inspiration proc DamageScript preimage breakdown is inconsistent");
      }
      seenBreakdowns.add(key);
    }
    if (seenBreakdowns.size !== expectedDamageScriptBreakdown.size ||
      join.damage_script_preimage_breakdown.reduce(
        (sum, row) => sum + Number(row.events), 0,
      ) !== Number(join.events)) {
      throw new Error("Inspiration proc DamageScript preimage breakdown does not conserve events");
    }
  }
  if (hasStageInputFreshnessBreakdown) {
    const freshnessCountFields = [
      "identity_groups", "events", "complete_stage_inputs", "compatible_candidate_events",
      "exact_stage_independent_events", "unresolved_stage_or_rounding_events",
      "events_without_compatible_candidates",
    ];
    const seenFreshnessRows = new Set();
    for (const row of join.stage_input_freshness_breakdown) {
      const key = stableStringify([
        String(row?.path ?? ""),
        String(row?.observation_state ?? ""),
        String(row?.oldest_age_bucket ?? ""),
        row?.all_wire_provenance === true,
        row?.all_same_wire_as_damage === true,
      ]);
      const expectedRow = expectedStageInputFreshnessBreakdown.get(key);
      if (!expectedRow || seenFreshnessRows.has(key) || row?.formula_authority !== false ||
        typeof row?.all_wire_provenance !== "boolean" ||
        typeof row?.all_same_wire_as_damage !== "boolean" ||
        freshnessCountFields.some((field) =>
          !Number.isSafeInteger(Number(row?.[field])) || Number(row[field]) < 0 ||
          Number(row[field]) !== Number(expectedRow[field]))) {
        throw new Error("Inspiration proc stage-input freshness breakdown is inconsistent");
      }
      seenFreshnessRows.add(key);
    }
    if (seenFreshnessRows.size !== expectedStageInputFreshnessBreakdown.size ||
      join.stage_input_freshness_breakdown.reduce(
        (sum, row) => sum + Number(row.events), 0,
      ) !== Number(join.events)) {
      throw new Error("Inspiration proc stage-input freshness breakdown does not conserve events");
    }
  }
  return {
    identityGroups: join.groups.length,
    uniqueRowEvents: totals.unique,
    ambiguousRowEvents: totals.ambiguous,
    missingRowEvents: totals.missing,
    resolvedScriptEvents: totals.resolvedScript,
    unresolvedScriptEvents: totals.unresolvedScript,
    uniqueAbilityDiagnosticEvents: totals.uniqueAbilityDiagnostic,
    uniqueAbilityResolvedScriptDiagnosticEvents: totals.uniqueAbilityResolvedScript,
    remainingWithoutExactOrUniqueEvents: totals.remainingWithoutExactOrUnique,
    damageScriptPreimageBreakdownRows: hasDamageScriptPreimageBreakdown
      ? join.damage_script_preimage_breakdown.length
      : 0,
    stageInputFreshnessBreakdownRows: hasStageInputFreshnessBreakdown
      ? join.stage_input_freshness_breakdown.length
      : 0,
  };
}

function buildCounterfactualFrontierResults(frontier, build) {
  if (!frontier) return [];
  if (![3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17].includes(Number(frontier.schema_version)) ||
    frontier.generated_by !== "rlogs-bpsr-status-effect-counterfactual-proof") {
    throw new Error("Unsupported status-effect counterfactual frontier schema or generator");
  }
  requireBuild(frontier.game_build, build, "status-effect counterfactual frontier");
  if (frontier.policy?.runtime_authority !== false ||
    frontier.policy?.formula_authority !== false ||
    frontier.policy?.unresolved_evidence_is_hidden !== false) {
    throw new Error("Status-effect counterfactual frontier authority policy is unsafe");
  }
  if (Number(frontier.schema_version) >= 5 &&
    frontier.policy?.candidate_projection_authority !== false) {
    throw new Error("Status-effect counterfactual frontier candidate policy is unsafe");
  }
  if (Number(frontier.schema_version) >= 6 &&
    frontier.policy?.near_controlled_diagnostic_authority !== false) {
    throw new Error("Status-effect counterfactual near-target policy is unsafe");
  }
  if (Number(frontier.schema_version) >= 7 &&
    frontier.policy?.near_controlled_source_attribute_diagnostic_authority !== false) {
    throw new Error("Status-effect counterfactual near-source policy is unsafe");
  }
  if (Number(frontier.schema_version) >= 8 &&
    (!String(frontier.policy?.exact_mode ?? "").includes("scene") ||
      !String(frontier.policy?.exact_mode ?? "").includes("actor identity") ||
      frontier.processing?.measured_peak_within_configured_limit !== true ||
      !Number.isSafeInteger(Number(frontier.processing?.memory_limit_mib)) ||
      Number(frontier.processing.memory_limit_mib) <= 0 ||
      !Number.isSafeInteger(Number(frontier.processing?.measured_peak_working_set_bytes)) ||
      Number(frontier.processing.measured_peak_working_set_bytes) <= 0 ||
      Number(frontier.processing.measured_peak_working_set_bytes) >
        Number(frontier.processing.memory_limit_mib) * 1024 * 1024 ||
      !Number.isSafeInteger(Number(frontier.processing?.partition_count)) ||
      Number(frontier.processing.partition_count) <= 0 ||
      !Array.isArray(frontier.processing?.selected_effect_ids) ||
      frontier.processing.selected_effect_ids.length === 0)) {
    throw new Error("Status-effect counterfactual schema-8 identity or memory policy is unsafe");
  }
  const crossEntityAudit = Number(frontier.schema_version) >= 9
    ? validateCrossEntityCounterfactualAudit(frontier)
    : null;
  if (!Number.isSafeInteger(Number(frontier.input?.bytes)) || Number(frontier.input.bytes) <= 0 ||
    !/^sha256:[0-9a-f]{64}$/.test(String(frontier.input?.sha256 ?? "")) ||
    !Array.isArray(frontier.input?.source_inputs) || frontier.input.source_inputs.length === 0) {
    throw new Error("Status-effect counterfactual frontier input provenance is incomplete");
  }

  const seen = new Set();
  const results = (frontier.effects ?? []).map((effect) => {
    const effectId = Number(effect.effect_id);
    const locus = String(effect.locus ?? "");
    if (!Number.isSafeInteger(effectId) || effectId <= 0 || !["source", "target"].includes(locus)) {
      throw new Error("Status-effect counterfactual frontier has an invalid effect locus");
    }
    const key = `${locus}:${effectId}`;
    if (seen.has(key)) throw new Error(`Duplicate status-effect counterfactual locus ${key}`);
    seen.add(key);
    if (Number(frontier.schema_version) >= 8 &&
      !frontier.processing.selected_effect_ids.map(Number).includes(effectId)) {
      throw new Error(`Status-effect counterfactual frontier ${key} was not explicitly selected`);
    }

    const exact = effect.exact_recorded_inputs ?? {};
    validateBladeSweepCandidateProjection(
      exact.blade_sweep_candidate_projection,
      effectId,
      locus,
      Number(frontier.schema_version),
    );
    const controlledGroups = counterfactualCount(exact.controlled_groups, `${key} controlled_groups`);
    const divergentGroups = counterfactualCount(exact.divergent_output_groups, `${key} divergent_output_groups`);
    if (divergentGroups > controlledGroups) {
      throw new Error(`Status-effect counterfactual frontier ${key} has more divergent than controlled groups`);
    }
    const nearSource = Number(frontier.schema_version) >= 7
      ? (frontier.near_controlled_source_attribute_diagnostic ?? []).find((row) =>
          Number(row?.effect_id) === effectId && String(row?.locus ?? "") === locus
        ) ?? null
      : null;
    if (nearSource) validateNearSourceCounterfactualAudit(nearSource, effectId, locus);
    for (const variant of effect.variants ?? []) {
      if (Number(variant.status?.effect_id) !== effectId) {
        throw new Error(`Status-effect counterfactual frontier ${key} contains a mismatched variant effect`);
      }
    }

    const status = divergentGroups > 0
      ? "controlled-delta-observed-proof-open"
      : controlledGroups > 0
        ? "controlled-equal-output-observed-proof-open"
        : "no-controlled-counterfactual-pair";
    const blockers = [
      "counterfactual-frontier-declares-no-formula-authority",
      "counterfactual-frontier-declares-no-runtime-authority",
      ...(controlledGroups === 0 ? ["exact-controlled-counterfactual-pair-missing"] : []),
      ...(nearSource && Number(nearSource.candidate_absent_near_pairs) === 0
        ? ["near-source-controlled-counterfactual-pair-missing"]
        : []),
      ...(controlledGroups > 0 && divergentGroups === 0 ? ["controlled-pair-has-no-observed-damage-delta"] : []),
      ...(divergentGroups > 0 ? [
        "general-formula-replication-and-domain-unproven",
        "effect-to-runtime-component-binding-unproven",
        "provider-player-ownership-unproven",
        "exact-transform-stacking-operation-order-and-rounding-unproven",
        "canonical-conservation-replay-unproven",
      ] : []),
    ];
    return {
      effect_id: String(effectId),
      locus,
      status,
      formula_authority: false,
      runtime_authority: false,
      blockers: uniqueSorted(blockers),
      observation: structuredClone(effect.observation ?? {}),
      exact_recorded_inputs: structuredClone(exact),
      target_current_hp_excluded_diagnostic: structuredClone(
        effect.target_current_hp_excluded_diagnostic ?? {},
      ),
      ...(nearSource ? {
        near_controlled_source_attribute_diagnostic: structuredClone(nearSource),
      } : {}),
      ...(Number(frontier.schema_version) >= 8 ? {
        counterfactual_frontier_schema_version: Number(frontier.schema_version),
        counterfactual_frontier_processing: structuredClone(frontier.processing),
      } : {}),
      ...(crossEntityAudit ? {
        cross_entity_baseline_proof: structuredClone(crossEntityAudit.baselineProof),
        cross_entity_diagnostics: structuredClone(crossEntityAudit.byKey.get(key)),
        structurally_absent_remote_skill_cast_packets_required: false,
      } : {}),
      variants: structuredClone(effect.variants ?? []),
    };
  });
  results.sort((left, right) =>
    compareIdentifiers(left.effect_id, right.effect_id) || compareText(left.locus, right.locus)
  );

  const exactControlledGroups = results.reduce(
    (sum, entry) => sum + Number(entry.exact_recorded_inputs.controlled_groups ?? 0),
    0,
  );
  const exactDivergentGroups = results.reduce(
    (sum, entry) => sum + Number(entry.exact_recorded_inputs.divergent_output_groups ?? 0),
    0,
  );
  if (Number(frontier.summary?.distinct_effect_loci) !== results.length ||
    Number(frontier.summary?.exact_controlled_groups) !== exactControlledGroups ||
    Number(frontier.summary?.exact_divergent_output_groups) !== exactDivergentGroups) {
    throw new Error("Status-effect counterfactual frontier summary does not match its effect loci");
  }
  return results;
}

function validateCrossEntityCounterfactualAudit(frontier) {
  const schemaVersion = Number(frontier.schema_version);
  const baselineProof = frontier.cross_entity_baseline_proof;
  if (Number(baselineProof?.schema_version) !== 8 ||
    !String(baselineProof?.path ?? "") ||
    !Number.isSafeInteger(Number(baselineProof?.bytes)) || Number(baselineProof.bytes) <= 0 ||
    !/^sha256:[0-9a-f]{64}$/.test(String(baselineProof?.sha256 ?? "")) ||
    frontier.policy?.structurally_absent_remote_skill_cast_packets_required !== false ||
    frontier.processing?.cross_entity_formula_state_diagnostic_enabled !== true ||
    frontier.processing?.cross_entity_measured_peak_within_configured_limit !== true ||
    !Number.isSafeInteger(Number(frontier.processing?.cross_entity_partition_count)) ||
    Number(frontier.processing.cross_entity_partition_count) <= 0 ||
    !Number.isSafeInteger(Number(frontier.processing?.largest_cross_entity_partition_bytes)) ||
    Number(frontier.processing.largest_cross_entity_partition_bytes) <= 0 ||
    !Number.isSafeInteger(Number(frontier.processing?.cross_entity_measured_peak_working_set_bytes)) ||
    Number(frontier.processing.cross_entity_measured_peak_working_set_bytes) <= 0 ||
    Number(frontier.processing.cross_entity_measured_peak_working_set_bytes) >
      Number(frontier.processing.memory_limit_mib) * 1024 * 1024) {
    throw new Error("Status-effect cross-entity counterfactual provenance or memory evidence is unsafe");
  }
  const selectedAttributeIds = frontier.processing?.selected_source_transition_attribute_ids;
  if (!Array.isArray(selectedAttributeIds) || selectedAttributeIds.length === 0 ||
    selectedAttributeIds.some((value) => !Number.isSafeInteger(Number(value)) || Number(value) <= 0) ||
    new Set(selectedAttributeIds.map(Number)).size !== selectedAttributeIds.length) {
    throw new Error("Status-effect cross-entity source-transition attribute selection is invalid");
  }

  const expectedKeys = new Set((frontier.effects ?? []).map((effect) =>
    `${String(effect?.locus ?? "")}:${Number(effect?.effect_id)}`
  ));
  const byKey = new Map([...expectedKeys].map((key) => [key, {}]));
  const specifications = [
    {
      minimumSchema: 9,
      field: "cross_entity_formula_state_diagnostic",
      policyField: "cross_entity_formula_state_diagnostic",
      summaryControlled: "cross_entity_formula_state_controlled_groups",
      summaryDivergent: "cross_entity_formula_state_divergent_groups",
      controlledField: "controlled_groups",
      divergentField: "divergent_output_groups",
      kind: "formula-state",
    },
    {
      minimumSchema: 10,
      field: "cross_entity_source_transition_diagnostic",
      policyField: "cross_entity_source_transition_diagnostic",
      summaryControlled: "cross_entity_source_transition_controlled_pairs",
      summaryDivergent: "cross_entity_source_transition_divergent_pairs",
      controlledField: "controlled_pairs",
      divergentField: "divergent_output_pairs",
      kind: "source-transition",
    },
    {
      minimumSchema: 11,
      field: "cross_entity_source_transition_target_current_hp_excluded_diagnostic",
      policyField: "cross_entity_source_transition_target_current_hp_excluded_diagnostic",
      summaryControlled:
        "cross_entity_source_transition_target_current_hp_excluded_controlled_pairs",
      summaryDivergent:
        "cross_entity_source_transition_target_current_hp_excluded_divergent_pairs",
      controlledField: "controlled_pairs",
      divergentField: "divergent_output_pairs",
      kind: "source-transition",
    },
    {
      minimumSchema: 12,
      field:
        "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic",
      policyField:
        "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic",
      summaryControlled:
        "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_controlled_pairs",
      summaryDivergent:
        "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_divergent_pairs",
      controlledField: "controlled_pairs",
      divergentField: "divergent_output_pairs",
      kind: "source-transition",
    },
    {
      minimumSchema: 13,
      field:
        "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic",
      policyField:
        "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic",
      summaryControlled:
        "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_controlled_pairs",
      summaryDivergent:
        "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_divergent_pairs",
      controlledField: "controlled_pairs",
      divergentField: "divergent_output_pairs",
      kind: "source-transition",
    },
  ];
  for (const specification of specifications) {
    if (schemaVersion < specification.minimumSchema) continue;
    if (!String(frontier.policy?.[specification.policyField] ?? "").includes("diagnostic") ||
      frontier.policy?.[`${specification.policyField}_authority`] !== false) {
      throw new Error(`Status-effect ${specification.field} policy is unsafe`);
    }
    const rows = frontier[specification.field];
    if (!Array.isArray(rows)) {
      throw new Error(`Status-effect ${specification.field} rows are missing`);
    }
    const seen = new Set();
    let controlled = 0;
    let divergent = 0;
    for (const row of rows) {
      const effectId = Number(row?.effect_id);
      const locus = String(row?.locus ?? "");
      const key = `${locus}:${effectId}`;
      if (!Number.isSafeInteger(effectId) || effectId <= 0 ||
        !["source", "target"].includes(locus) || !expectedKeys.has(key) || seen.has(key) ||
        row?.formula_authority !== false || row?.runtime_authority !== false ||
        row?.ui_display_authority !== false || row?.provider_rdps_credit_allowed !== false) {
        throw new Error(`Status-effect ${specification.field} has an unsafe effect row`);
      }
      seen.add(key);
      if (specification.kind === "formula-state") {
        validateCrossEntityFormulaStateRow(row, effectId);
      } else {
        validateCrossEntitySourceTransitionRow(
          row,
          effectId,
          selectedAttributeIds.map(Number),
          schemaVersion,
        );
      }
      controlled += counterfactualRequiredCount(
        row,
        specification.controlledField,
        `${specification.field} ${key}`,
      );
      divergent += counterfactualRequiredCount(
        row,
        specification.divergentField,
        `${specification.field} ${key}`,
      );
      byKey.get(key)[specification.field] = structuredClone(row);
    }
    if (controlled !== counterfactualRequiredCount(
      frontier.summary,
      specification.summaryControlled,
      "counterfactual summary",
    ) || divergent !== counterfactualRequiredCount(
      frontier.summary,
      specification.summaryDivergent,
      "counterfactual summary",
    )) {
      throw new Error(`Status-effect ${specification.field} totals do not conserve`);
    }
  }
  if (schemaVersion >= 15) {
    if (!String(frontier.policy?.cross_entity_source_status_transition_review_band_diagnostic ?? "")
        .includes("remain rejected") ||
      frontier.policy?.cross_entity_source_status_transition_review_band_diagnostic_authority !== false) {
      throw new Error("Status-effect cross-entity source-status review-band policy is unsafe");
    }
    const reviewRows = frontier
      .cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic;
    const totals = (reviewRows ?? []).flatMap((row) => row.variants ?? []).reduce(
      (sum, variant) => ({
        pairs: sum.pairs + Number(variant.source_status_transition_review_band_pairs),
        noSourceTransition: sum.noSourceTransition + Number(
          variant.source_status_transition_review_band_pairs_without_source_attribute_transition ?? 0,
        ),
        unselectedSourceTransition: sum.unselectedSourceTransition + Number(
          variant.source_status_transition_review_band_pairs_with_unselected_source_attribute_transition ?? 0,
        ),
        sourceCompatible: sum.sourceCompatible + Number(
          variant.source_status_transition_review_band_pairs_with_selected_source_attribute_transition,
        ),
        otherwiseCompatible: sum.otherwiseCompatible + Number(
          variant
            .source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit,
        ),
      }),
      {
        pairs: 0,
        noSourceTransition: 0,
        unselectedSourceTransition: 0,
        sourceCompatible: 0,
        otherwiseCompatible: 0,
      },
    );
    if (totals.pairs !== counterfactualRequiredCount(
      frontier.summary,
      "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs",
      "counterfactual summary",
    ) || (schemaVersion >= 16 && totals.noSourceTransition !== counterfactualRequiredCount(
      frontier.summary,
      "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_without_source_attribute_transition",
      "counterfactual summary",
    )) || (schemaVersion >= 16 && totals.unselectedSourceTransition !== counterfactualRequiredCount(
      frontier.summary,
      "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_unselected_source_attribute_transition",
      "counterfactual summary",
    )) || totals.sourceCompatible !== counterfactualRequiredCount(
      frontier.summary,
      "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_transition",
      "counterfactual summary",
    ) || totals.otherwiseCompatible !== counterfactualRequiredCount(
      frontier.summary,
      "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit",
      "counterfactual summary",
    )) {
      throw new Error("Status-effect cross-entity source-status review-band totals do not conserve");
    }
  }
  return { baselineProof, byKey };
}

function validateCrossEntityFormulaStateRow(row, effectId) {
  if (!Array.isArray(row?.variants) || row.variants.length === 0) {
    throw new Error("Status-effect cross-entity formula-state variants are missing");
  }
  let controlled = 0;
  let divergent = 0;
  for (const variant of row.variants) {
    validateCrossEntityStatus(variant?.status, effectId);
    const counts = Object.fromEntries([
      "present_groups",
      "present_samples",
      "absent_formula_state_unobserved_groups",
      "controlled_groups",
      "sample_comparisons",
      "deterministic_groups",
      "equal_output_groups",
      "divergent_output_groups",
      "nondeterministic_groups",
    ].map((field) => [field, counterfactualRequiredCount(variant, field, "formula-state variant")]));
    if (counts.absent_formula_state_unobserved_groups > counts.present_groups ||
      counts.deterministic_groups + counts.nondeterministic_groups > counts.controlled_groups ||
      counts.equal_output_groups + counts.divergent_output_groups > counts.deterministic_groups ||
      !Array.isArray(variant?.divergent_examples) ||
      variant.divergent_examples.length > counts.divergent_output_groups) {
      throw new Error("Status-effect cross-entity formula-state variant does not conserve");
    }
    controlled += counts.controlled_groups;
    divergent += counts.divergent_output_groups;
  }
  if (controlled !== counterfactualRequiredCount(row, "controlled_groups", "formula-state row") ||
    divergent !== counterfactualRequiredCount(row, "divergent_output_groups", "formula-state row")) {
    throw new Error("Status-effect cross-entity formula-state row totals do not conserve");
  }
}

function validateCrossEntitySourceTransitionRow(row, effectId, selectedAttributeIds, schemaVersion) {
  if (!Array.isArray(row?.selected_source_attribute_ids) ||
    stableStringify(row.selected_source_attribute_ids.map(Number)) !==
      stableStringify(selectedAttributeIds) ||
    !Array.isArray(row?.variants) || row.variants.length === 0) {
    throw new Error("Status-effect cross-entity source-transition selection is inconsistent");
  }
  const selected = new Set(selectedAttributeIds);
  let controlled = 0;
  let divergent = 0;
  for (const variant of row.variants) {
    validateCrossEntityStatus(variant?.status, effectId);
    const countFields = [
      "candidate_present_groups",
      "present_groups_without_absent_status_state",
      "candidate_absent_formula_state_pairs",
      "rejected_without_source_attribute_transition",
      "rejected_with_unselected_source_attribute_transition",
      "controlled_pairs",
      "sample_comparisons",
      "deterministic_pairs",
      "equal_output_pairs",
      "divergent_output_pairs",
      "nondeterministic_pairs",
      ...(schemaVersion >= 12 ? [
        "rejected_with_excess_target_status_co_transitions",
        "target_status_transition_pairs",
      ] : []),
      ...(schemaVersion >= 13 ? [
        "rejected_with_excess_source_status_co_transitions",
        "source_status_transition_pairs",
      ] : []),
      ...(schemaVersion >= 15 ? [
        "source_status_transition_review_band_pairs",
        "source_status_transition_review_band_pairs_with_selected_source_attribute_transition",
        "source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit",
      ] : []),
      ...(schemaVersion >= 16 ? [
        "source_status_transition_review_band_pairs_without_source_attribute_transition",
        "source_status_transition_review_band_pairs_with_unselected_source_attribute_transition",
      ] : []),
    ];
    const counts = Object.fromEntries(countFields.map((field) => [
      field,
      counterfactualRequiredCount(variant, field, "source-transition variant"),
    ]));
    if (counts.present_groups_without_absent_status_state > counts.candidate_present_groups ||
      counts.deterministic_pairs + counts.nondeterministic_pairs > counts.controlled_pairs ||
      counts.equal_output_pairs + counts.divergent_output_pairs > counts.deterministic_pairs ||
      !Array.isArray(variant?.examples) || variant.examples.length > counts.controlled_pairs) {
      throw new Error("Status-effect cross-entity source-transition variant does not conserve");
    }
    if (schemaVersion >= 14) {
      const histogram = variant?.source_status_transition_distance_counts;
      if (!Array.isArray(histogram) ||
        (counts.candidate_absent_formula_state_pairs > 0 && histogram.length === 0) ||
        histogram.some((entry, index) =>
          !Number.isSafeInteger(Number(entry?.transition_distance)) ||
          Number(entry.transition_distance) < 0 ||
          !Number.isSafeInteger(Number(entry?.pairs)) || Number(entry.pairs) <= 0 ||
          (index > 0 && Number(histogram[index - 1].transition_distance) >=
            Number(entry.transition_distance))) ||
        histogram.reduce((sum, entry) => sum + Number(entry.pairs), 0) !==
          counts.candidate_absent_formula_state_pairs) {
        throw new Error("Status-effect cross-entity source-status distance histogram does not conserve");
      }
    }
    if (schemaVersion >= 15 &&
      (counts.source_status_transition_review_band_pairs >
          counts.rejected_with_excess_source_status_co_transitions ||
        counts.source_status_transition_review_band_pairs_with_selected_source_attribute_transition >
          counts.source_status_transition_review_band_pairs ||
        counts
          .source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit >
          counts.source_status_transition_review_band_pairs_with_selected_source_attribute_transition)) {
      throw new Error("Status-effect cross-entity source-status review band is inconsistent");
    }
    if (schemaVersion >= 16 &&
      counts.source_status_transition_review_band_pairs_without_source_attribute_transition +
        counts.source_status_transition_review_band_pairs_with_unselected_source_attribute_transition +
        counts.source_status_transition_review_band_pairs_with_selected_source_attribute_transition !==
          counts.source_status_transition_review_band_pairs) {
      throw new Error("Status-effect cross-entity source-status review-band source transitions do not conserve");
    }
    for (const example of variant.examples) {
      if (!Array.isArray(example?.source_attribute_transitions) ||
        example.source_attribute_transitions.length === 0 ||
        example.source_attribute_transitions.some((transition) =>
          !selected.has(Number(transition?.attribute_id)) ||
          !Number.isSafeInteger(Number(transition?.present_value)) ||
          !Number.isSafeInteger(Number(transition?.absent_value))) ||
        !example?.present_formula_context || !example?.absent_formula_context) {
        throw new Error("Status-effect cross-entity source-transition example is incomplete");
      }
      if (schemaVersion >= 12) {
        validateStatusTransitionDistance(example, "target");
      }
      if (schemaVersion >= 13) {
        validateStatusTransitionDistance(example, "source");
      }
    }
    controlled += counts.controlled_pairs;
    divergent += counts.divergent_output_pairs;
  }
  if (controlled !== counterfactualRequiredCount(row, "controlled_pairs", "source-transition row") ||
    divergent !== counterfactualRequiredCount(row, "divergent_output_pairs", "source-transition row")) {
    throw new Error("Status-effect cross-entity source-transition row totals do not conserve");
  }
}

function validateStatusTransitionDistance(example, locus) {
  const present = example?.[`${locus}_status_present_only_co_transitions`];
  const absent = example?.[`${locus}_status_absent_only_co_transitions`];
  const distance = Number(example?.[`${locus}_status_transition_distance`]);
  if (!Array.isArray(present) || !Array.isArray(absent) ||
    !Number.isSafeInteger(distance) || distance < 0 ||
    distance !== present.length + absent.length || distance > 4) {
    throw new Error(`Status-effect cross-entity ${locus}-status transition example is invalid`);
  }
}

function validateCrossEntityStatus(status, effectId) {
  if (Number(status?.effect_id) !== effectId ||
    !String(status?.provider_relationship ?? "") ||
    typeof status?.provider_attribute_state_observed !== "boolean" ||
    (status.provider_attribute_state_observed === true &&
      !Number.isSafeInteger(Number(status?.provider_attribute_state_id))) ||
    (status.provider_attribute_state_observed === false &&
      status?.provider_attribute_state_id != null)) {
    throw new Error("Status-effect cross-entity normalized status identity is invalid");
  }
}

function counterfactualRequiredCount(object, field, label) {
  if (!Object.prototype.hasOwnProperty.call(object ?? {}, field)) {
    throw new Error(`Status-effect ${label} is missing ${field}`);
  }
  return counterfactualCount(object[field], `${label} ${field}`);
}

function validateNearSourceCounterfactualAudit(row, effectId, locus) {
  const countFields = [
    "candidate_absent_near_pairs",
    "divergent_output_pairs",
  ];
  if (Number(row?.effect_id) !== effectId || String(row?.locus ?? "") !== locus ||
    !Array.isArray(row?.selected_source_attribute_ids) ||
    row.selected_source_attribute_ids.length === 0 ||
    new Set(row.selected_source_attribute_ids.map(Number)).size !==
      row.selected_source_attribute_ids.length ||
    row.selected_source_attribute_ids.some((value) =>
      !Number.isSafeInteger(Number(value)) || Number(value) <= 0) ||
    countFields.some((field) =>
      !Number.isSafeInteger(Number(row?.[field])) || Number(row[field]) < 0) ||
    (row.minimum_transition_distance != null &&
      (!Number.isSafeInteger(Number(row.minimum_transition_distance)) ||
        Number(row.minimum_transition_distance) <= 0)) ||
    !Array.isArray(row?.variants) || row.variants.length === 0 ||
    row?.formula_authority !== false || row?.runtime_authority !== false ||
    row?.provider_rdps_credit_allowed !== false) {
    throw new Error("Status-effect near-source counterfactual audit is unsafe");
  }
  let candidatePairs = 0;
  let divergentPairs = 0;
  for (const variant of row.variants) {
    const variantCounts = [
      "candidate_present_groups",
      "present_groups_without_effect_absent_identity_state",
      "effect_absent_identity_state_candidates",
      "rejected_without_source_attribute_transition",
      "rejected_with_unselected_source_attribute_transition",
      "rejected_with_excess_source_status_co_transitions",
      "candidate_absent_near_pairs",
      "sample_comparisons",
      "deterministic_pairs",
      "equal_output_pairs",
      "divergent_output_pairs",
      "nondeterministic_pairs",
    ];
    if (Number(variant?.status?.effect_id) !== effectId ||
      variantCounts.some((field) =>
        !Number.isSafeInteger(Number(variant?.[field])) || Number(variant[field]) < 0) ||
      Number(variant.present_groups_without_effect_absent_identity_state) >
        Number(variant.candidate_present_groups) ||
      Number(variant.candidate_absent_near_pairs) >
        Number(variant.effect_absent_identity_state_candidates) ||
      Number(variant.equal_output_pairs) + Number(variant.divergent_output_pairs) >
        Number(variant.deterministic_pairs) ||
      !Array.isArray(variant?.rejected_source_attribute_transition_sets) ||
      !Array.isArray(variant?.transition_distance_counts) ||
      !Array.isArray(variant?.examples)) {
      throw new Error("Status-effect near-source counterfactual variant is inconsistent");
    }
    candidatePairs += Number(variant.candidate_absent_near_pairs);
    divergentPairs += Number(variant.divergent_output_pairs);
  }
  if (candidatePairs !== Number(row.candidate_absent_near_pairs) ||
    divergentPairs !== Number(row.divergent_output_pairs)) {
    throw new Error("Status-effect near-source counterfactual totals do not conserve");
  }
  return row;
}

function counterfactualCount(value, label) {
  const count = Number(value ?? 0);
  if (!Number.isSafeInteger(count) || count < 0) {
    throw new Error(`Status-effect counterfactual frontier ${label} must be a non-negative integer`);
  }
  return count;
}

function validateBladeSweepCandidateProjection(candidate, effectId, locus, schemaVersion) {
  if (schemaVersion < 5) return;
  const applicable = effectId === 2110092 && locus === "target";
  if (!applicable) {
    if (candidate !== null && candidate !== undefined) {
      throw new Error(`Unexpected Blade Sweep candidate projection on ${locus}:${effectId}`);
    }
    return;
  }
  const controlled = counterfactualCount(
    candidate?.controlled_divergent_groups,
    "Blade Sweep candidate controlled_divergent_groups",
  );
  const withDefense = counterfactualCount(
    candidate?.groups_with_target_physical_defense,
    "Blade Sweep candidate groups_with_target_physical_defense",
  );
  const missingDefense = counterfactualCount(
    candidate?.groups_missing_target_physical_defense,
    "Blade Sweep candidate groups_missing_target_physical_defense",
  );
  const invalid = counterfactualCount(
    candidate?.groups_with_invalid_nonnegative_inputs,
    "Blade Sweep candidate groups_with_invalid_nonnegative_inputs",
  );
  const variants = candidate?.variants;
  if (candidate?.model_id !==
      "effect-2110092-pre-mitigation-650bp-defense-reduction-candidate" ||
    Number(candidate?.effect_id) !== 2110092 ||
    Number(candidate?.armor_penetration_basis_points) !== 650 ||
    Number(candidate?.defense_curve_constant) !== 22000 ||
    controlled !== withDefense + missingDefense + invalid ||
    !Array.isArray(variants) ||
    JSON.stringify(variants.map((row) => String(row.rounding))) !==
      JSON.stringify(["floor", "ceil", "round-half-up"]) ||
    variants.some((row) =>
      counterfactualCount(row.compatible_groups, "candidate compatible_groups") +
        counterfactualCount(row.rejected_groups, "candidate rejected_groups") !== withDefense) ||
    !Array.isArray(candidate?.examples) || candidate.examples.length > withDefense ||
    candidate?.candidate_selected !== false ||
    candidate?.exact_damage_projection_proven !== false ||
    candidate?.exact_operation_order_proven !== false ||
    candidate?.exact_integer_rounding_proven !== false ||
    candidate?.formula_authority !== false || candidate?.runtime_authority !== false ||
    candidate?.ui_display_authority !== false ||
    candidate?.provider_rdps_credit_allowed !== false) {
    throw new Error("Unsafe Blade Sweep counterfactual candidate projection");
  }
}

function buildProviderOwnershipIndex(proofs, proofPaths, build) {
  const index = new Map();
  for (let proofIndex = 0; proofIndex < proofs.length; proofIndex += 1) {
    const proof = proofs[proofIndex];
    if (![2, 3, 4, 5].includes(proof.schema_version) ||
      proof.tool !== "rlogs-bpsr-status-effect-provider-ownership-proof") {
      throw new Error("Unsupported status-effect provider ownership proof schema or generator");
    }
    requireBuild(proof.game_build, build, "status-effect provider ownership proof");
    if (proof.policy?.scope !== "provider_ownership_only" ||
      proof.policy?.exact_numeric_effect_ids_authoritative !== true ||
      proof.policy?.exact_input_build_authoritative !== true ||
      proof.policy?.localized_names_are_evidence_only !== true ||
      proof.policy?.actor_kind_or_packet_proven_ancestry_required_for_player_ownership !== true ||
      proof.policy?.future_actor_snapshots_may_backfill_prior_status_events !== false ||
      proof.policy?.unknown_and_unresolved_events_preserved !== true ||
      proof.policy?.formula_authority !== false ||
      proof.policy?.runtime_authority !== false ||
      proof.policy?.provider_rdps_credit_allowed !== false) {
      throw new Error("Status-effect provider ownership proof authority policy is unsafe");
    }
    if (proof.schema_version >= 3 &&
      (proof.policy?.bpsr_player_entity_uuid_character_id_contract_applied !== true ||
        proof.policy?.explicit_and_derived_character_id_mismatches_are_rejected !== true)) {
      throw new Error("Status-effect provider ownership proof stable-character policy is unsafe");
    }
    if (proof.schema_version >= 4 &&
      (proof.policy?.prior_exact_status_instance_player_ownership_may_flow_forward !== true ||
        proof.policy
          ?.forward_status_instance_ownership_requires_exact_run_target_effect_instance_and_source !== true ||
        proof.policy?.conflicting_status_instance_owners_disable_inheritance !== true)) {
      throw new Error("Status-effect provider ownership proof forward-instance policy is unsafe");
    }
    if (proof.schema_version >= 5 &&
      (proof.policy?.later_attributed_combat_relation_in_same_exact_wire_packet_may_resolve_provider !== true ||
        proof.policy
          ?.same_wire_packet_resolution_requires_exact_capture_connection_stream_and_observed_time !== true)) {
      throw new Error("Status-effect provider ownership proof same-wire-packet policy is unsafe");
    }
    const inputs = proof.inputs ?? [];
    if (!Array.isArray(inputs) || inputs.length === 0) {
      throw new Error("Status-effect provider ownership proof has no exact inputs");
    }
    const inputRlogs = new Map();
    for (const input of inputs) {
      requireBuild(input.game_build, build, "status-effect provider ownership proof input");
      const bytes = Number(input.bytes);
      const sha256 = String(input.sha256 ?? "");
      const label = path.basename(String(input.path ?? "")).toLowerCase();
      if (!label || !Number.isSafeInteger(bytes) || bytes <= 0 ||
        !/^sha256:[0-9a-f]{64}$/.test(sha256) || inputRlogs.has(label)) {
        throw new Error("Status-effect provider ownership proof input provenance is incomplete or duplicated");
      }
      inputRlogs.set(label, { bytes, sha256 });
    }
    const effectReports = proof.effects ?? [];
    const resolutionEventCounts = new Map();
    const sourceEntities = new Map();
    for (const resolution of proof.resolutions ?? []) {
      const effectId = Number(resolution.effect_id);
      const events = counterfactualCount(
        resolution.status_events,
        `provider ownership effect ${effectId} resolution status_events`,
      );
      resolutionEventCounts.set(effectId, (resolutionEventCounts.get(effectId) ?? 0) + events);
      const sourceEntity = Number(resolution.source?.entity_uuid);
      if (Number.isSafeInteger(sourceEntity) && sourceEntity > 0) {
        (sourceEntities.get(effectId) ?? sourceEntities.set(effectId, new Set()).get(effectId)).add(String(sourceEntity));
      }
    }
    for (const effect of effectReports) {
      const effectId = Number(effect.effect_id);
      const statusEvents = counterfactualCount(
        effect.status_events,
        `provider ownership effect ${effectId} status_events`,
      );
      if (!Number.isSafeInteger(effectId) || effectId <= 0 || statusEvents <= 0 || index.has(effectId)) {
        throw new Error("Status-effect provider ownership proof has an invalid or duplicate effect result");
      }
      const resolutionCount = Object.values(effect.resolution_counts ?? {}).reduce(
        (sum, value) => sum + counterfactualCount(value, `provider ownership effect ${effectId} resolution count`),
        0,
      );
      if (resolutionCount !== statusEvents || resolutionEventCounts.get(effectId) !== statusEvents ||
        effect.player_actor_ownership_proven_for_every_sourced_event !== true) {
        throw new Error(`Status-effect provider ownership proof effect ${effectId} is incomplete`);
      }
      const directPlayerEvents = counterfactualCount(
        effect.resolution_counts?.direct_player,
        `provider ownership effect ${effectId} direct_player`,
      );
      const playerOwnerEvents = counterfactualCount(
        effect.resolution_counts?.owned_by_player,
        `provider ownership effect ${effectId} owned_by_player`,
      );
      const sameWirePacketOwnerEvents = counterfactualCount(
        effect.resolution_counts?.same_wire_packet_owned_by_player,
        `provider ownership effect ${effectId} same_wire_packet_owned_by_player`,
      );
      const priorStatusInstanceOwnerEvents = counterfactualCount(
        effect.resolution_counts?.prior_status_instance_player,
        `provider ownership effect ${effectId} prior_status_instance_player`,
      );
      const stableCharacterEvents = counterfactualCount(
        effect.status_events_with_stable_player_character_id,
        `provider ownership effect ${effectId} stable character events`,
      );
      const stablePlayerCharacterIds = uniqueSorted(
        (effect.proven_player_character_ids ?? []).map(String),
      );
      if (stablePlayerCharacterIds.some((characterId) => !/^\d+$/.test(characterId)) ||
        (effect.stable_player_character_id_proven_for_every_sourced_event === true &&
          stablePlayerCharacterIds.length === 0)) {
        throw new Error(`Status-effect provider ownership proof effect ${effectId} has invalid stable character identities`);
      }
      index.set(effectId, {
        effect_id: String(effectId),
        proof: fileDescriptor(proofPaths[proofIndex]),
        input_rlogs: inputRlogs,
        selected_status_events: statusEvents,
        direct_player_status_events: directPlayerEvents,
        player_owned_status_events: playerOwnerEvents,
        same_wire_packet_player_owned_status_events: sameWirePacketOwnerEvents,
        prior_status_instance_player_owned_status_events: priorStatusInstanceOwnerEvents,
        unique_source_entity_uuids: uniqueSorted([...(sourceEntities.get(effectId) ?? [])]),
        run_scoped_player_ownership_proven:
          directPlayerEvents + playerOwnerEvents + sameWirePacketOwnerEvents +
            priorStatusInstanceOwnerEvents === statusEvents,
        stable_player_character_id_events: stableCharacterEvents,
        stable_player_character_ids: stablePlayerCharacterIds,
        stable_player_character_id_proven_for_every_status_event:
          effect.stable_player_character_id_proven_for_every_sourced_event === true,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
      });
    }
  }
  return index;
}

function buildStatusEventSeasonContextIndex(proofs, proofPaths, build) {
  const index = new Map();
  const gapKindNames = [
    "capture_drop",
    "tcp_gap",
    "unknown_route",
    "decode_failure",
    "unsupported_fragment",
  ];
  const isExactWireSeasonObservation = (observation) =>
    Number.isSafeInteger(Number(observation?.wire_capture_sequence)) &&
    Number(observation.wire_capture_sequence) > 0 &&
    Number.isSafeInteger(Number(observation?.wire_connection_id)) &&
    Number(observation.wire_connection_id) > 0 &&
    Number.isSafeInteger(Number(observation?.wire_stream_id)) &&
    Number(observation.wire_stream_id) > 0;
  for (let proofIndex = 0; proofIndex < proofs.length; proofIndex += 1) {
    const proof = proofs[proofIndex];
    const seasonProofSchema = Number(proof?.schema_version);
    if (![1, 2, 3].includes(seasonProofSchema) ||
      proof?.tool !== "rlogs-bpsr-status-event-season-context-proof") {
      throw new Error("Unsupported status-event season-context proof schema or generator");
    }
    requireBuild(proof.game_build, build, "status-event season-context proof");
    if (proof.policy?.scope !== "event_time_season_context_only" ||
      proof.policy?.exact_numeric_effect_ids_authoritative !== true ||
      proof.policy?.exact_input_build_authoritative !== true ||
      proof.policy
        ?.only_positive_season_ids_from_bpsr_canonical_profile_events_are_accepted !== true ||
      proof.policy?.season_context_must_precede_status_in_same_sealed_rlog !== true ||
      proof.policy?.future_profile_events_may_backfill_earlier_status_events !== false ||
      proof.policy?.current_character_snapshots_may_replace_historical_context !== false ||
      (seasonProofSchema >= 3 &&
        proof.policy?.season_observations_require_exact_wire_provenance !== true) ||
      (seasonProofSchema >= 2 &&
        proof.policy
          ?.prior_continuous_monitor_context_is_candidate_until_protocol_coverage !== true) ||
      proof.policy?.unresolved_context_is_preserved !== true ||
      proof.policy?.formula_authority !== false || proof.policy?.runtime_authority !== false ||
      proof.policy?.provider_rdps_credit_allowed !== false) {
      throw new Error("Status-event season-context proof policy is unsafe");
    }
    const inputRlogs = new Map();
    for (const input of proof.inputs ?? []) {
      requireBuild(input.game_build, build, "status-event season-context proof input");
      const label = path.basename(String(input?.path ?? "")).toLowerCase();
      const bytes = Number(input?.bytes);
      const sha256 = String(input?.sha256 ?? "").toLowerCase();
      if (!label || !Number.isSafeInteger(bytes) || bytes <= 0 ||
        !/^sha256:[0-9a-f]{64}$/.test(sha256) || inputRlogs.has(label)) {
        throw new Error("Status-event season-context proof input provenance is incomplete or duplicated");
      }
      inputRlogs.set(label, { bytes, sha256 });
    }
    if (inputRlogs.size === 0) {
      throw new Error("Status-event season-context proof has no exact inputs");
    }
    const selectedEffectIds = uniqueSortedNumbers(proof.selection?.effect_ids ?? []);
    const events = proof.events ?? [];
    const selectedEffectSet = new Set(selectedEffectIds);
    if (!Array.isArray(events) || selectedEffectIds.length === 0 ||
      events.some((event) => !selectedEffectSet.has(Number(event?.effect_id))) ||
      events.length !== Number(proof.summary?.selected_status_events) ||
      events.length !== Number(proof.summary?.selected_events_with_prior_season_context) +
        Number(proof.summary?.selected_events_without_prior_season_context)) {
      throw new Error("Status-event season-context proof summary is inconsistent");
    }
    for (const effectId of selectedEffectIds) {
      const effectEvents = events.filter((event) => Number(event?.effect_id) === effectId);
      if (effectEvents.length === 0 || index.has(effectId)) {
        throw new Error(`Status-event season-context proof has invalid effect ${effectId}`);
      }
      let prior = 0;
      let laterOnly = 0;
      let noObservation = 0;
      let continuousMonitorCandidates = 0;
      let gapRoutesClassifiedCandidates = 0;
      let gapFreeSeasonSourceWireLaneCandidates = 0;
      let noTransportGapKindCandidates = 0;
      const gapKindTotals = Object.fromEntries(gapKindNames.map((name) => [name, 0]));
      const priorSeasonIds = new Set();
      const continuousMonitorSeasonIds = new Set();
      for (const event of effectEvents) {
        const priorContext = event?.prior_season_context;
        const laterContext = event?.first_later_season_observation;
        const hasPrior = priorContext !== null && priorContext !== undefined;
        const hasLater = laterContext !== null && laterContext !== undefined;
        const monitorCandidate = event?.prior_continuous_monitor_context_candidate;
        const hasMonitorCandidate = monitorCandidate !== null && monitorCandidate !== undefined;
        const gapKindCounts = monitorCandidate?.data_gap_kind_counts_since_observation;
        const gapKindKeys = gapKindCounts && typeof gapKindCounts === "object"
          ? Object.keys(gapKindCounts).sort()
          : [];
        const gapKindsAreClassified = seasonProofSchema >= 3 && hasMonitorCandidate &&
          stableStringify(gapKindKeys) === stableStringify([...gapKindNames].sort()) &&
          gapKindNames.every((name) =>
            Number.isSafeInteger(Number(gapKindCounts[name])) && Number(gapKindCounts[name]) >= 0
          );
        const classifiedGapTotal = gapKindsAreClassified
          ? gapKindNames.reduce((sum, name) => sum + Number(gapKindCounts[name]), 0)
          : null;
        const sourceLaneGaps = Number(
          monitorCandidate?.season_source_wire_lane_data_gaps_since_observation,
        );
        if (!Number.isSafeInteger(Number(event?.sequence)) || Number(event.sequence) <= 0 ||
          !Number.isSafeInteger(Number(event?.observed_micros)) ||
          Number(event.observed_micros) < 0 ||
          event?.season_context_proven_before_event !== hasPrior ||
          event?.future_backfill_rejected !== (!hasPrior && hasLater) ||
          (seasonProofSchema >= 2 &&
            event?.continuous_monitor_context_is_formula_authority !== false) ||
          (seasonProofSchema >= 3 && hasPrior && !isExactWireSeasonObservation(priorContext)) ||
          (seasonProofSchema >= 3 && hasLater && !isExactWireSeasonObservation(laterContext)) ||
          (hasPrior && (!Number.isSafeInteger(Number(priorContext.season_id)) ||
            Number(priorContext.season_id) <= 0 ||
            Number(priorContext.profile_sequence) >= Number(event.sequence) ||
            Number(priorContext.profile_observed_micros) > Number(event.observed_micros))) ||
          (hasLater && (!Number.isSafeInteger(Number(laterContext.season_id)) ||
            Number(laterContext.season_id) <= 0 ||
            Number(laterContext.profile_sequence) <= Number(event.sequence))) ||
          (hasMonitorCandidate &&
            (!Number.isSafeInteger(Number(monitorCandidate?.season?.season_id)) ||
              Number(monitorCandidate.season.season_id) <= 0 ||
              Number(monitorCandidate.season.profile_observed_micros) >
                Number(event.observed_micros) ||
              monitorCandidate.source_session_id === event.session_id ||
              monitorCandidate.consecutive_run_chain !== true ||
              monitorCandidate.monotonic_monitor_clock !== true ||
              monitorCandidate.protocol_event_coverage_required_for_authority !== true ||
              !Number.isSafeInteger(Number(monitorCandidate.data_gaps_since_observation)) ||
              Number(monitorCandidate.data_gaps_since_observation) < 0 ||
              (seasonProofSchema >= 3 &&
                (!isExactWireSeasonObservation(monitorCandidate.season) ||
                  !gapKindsAreClassified ||
                  classifiedGapTotal !== Number(monitorCandidate.data_gaps_since_observation) ||
                  !Number.isSafeInteger(sourceLaneGaps) || sourceLaneGaps < 0 ||
                  sourceLaneGaps > Number(monitorCandidate.data_gaps_since_observation) ||
                  monitorCandidate.season_source_wire_lane_gap_free !==
                    (sourceLaneGaps === 0) ||
                  monitorCandidate.no_capture_or_tcp_gap_kind_since_observation !==
                    (Number(gapKindCounts.capture_drop) + Number(gapKindCounts.tcp_gap) === 0)))))) {
          throw new Error(`Status-event season-context proof has unsafe event ${event?.sequence}`);
        }
        if (hasPrior) {
          prior += 1;
          priorSeasonIds.add(Number(priorContext.season_id));
        } else if (hasLater) {
          laterOnly += 1;
        } else {
          noObservation += 1;
        }
        if (hasMonitorCandidate) {
          continuousMonitorCandidates += 1;
          continuousMonitorSeasonIds.add(Number(monitorCandidate.season.season_id));
          if (seasonProofSchema >= 3) {
            gapRoutesClassifiedCandidates += 1;
            if (monitorCandidate.season_source_wire_lane_gap_free === true) {
              gapFreeSeasonSourceWireLaneCandidates += 1;
            }
            if (monitorCandidate.no_capture_or_tcp_gap_kind_since_observation === true) {
              noTransportGapKindCandidates += 1;
            }
            for (const name of gapKindNames) {
              gapKindTotals[name] += Number(gapKindCounts[name]);
            }
          }
        }
      }
      index.set(effectId, {
        effect_id: String(effectId),
        proof: fileDescriptor(proofPaths[proofIndex]),
        input_rlogs: inputRlogs,
        selected_status_events: effectEvents.length,
        events_with_prior_season_context: prior,
        events_with_only_later_season_observation: laterOnly,
        events_without_any_season_observation_in_rlog: noObservation,
        prior_season_ids: [...priorSeasonIds].sort((left, right) => left - right),
        prior_continuous_monitor_context_candidates: continuousMonitorCandidates,
        prior_continuous_monitor_season_ids:
          [...continuousMonitorSeasonIds].sort((left, right) => left - right),
        continuous_monitor_gap_routes_classified_candidates: gapRoutesClassifiedCandidates,
        gap_free_season_source_wire_lane_candidates: gapFreeSeasonSourceWireLaneCandidates,
        no_transport_gap_kind_candidates: noTransportGapKindCandidates,
        continuous_monitor_gap_kind_totals: gapKindTotals,
        every_continuous_monitor_candidate_has_classified_gap_routes:
          continuousMonitorCandidates > 0 &&
          gapRoutesClassifiedCandidates === continuousMonitorCandidates,
        every_selected_event_has_prior_continuous_monitor_context_candidate:
          continuousMonitorCandidates === effectEvents.length,
        every_selected_event_has_prior_season_context: prior === effectEvents.length,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
      });
    }
    const receipts = selectedEffectIds.map((effectId) => index.get(effectId));
    const priorTotal = receipts.reduce(
      (sum, receipt) => sum + receipt.events_with_prior_season_context,
      0,
    );
    const laterTotal = receipts.reduce(
      (sum, receipt) => sum + receipt.events_with_only_later_season_observation,
      0,
    );
    const absentTotal = receipts.reduce(
      (sum, receipt) => sum + receipt.events_without_any_season_observation_in_rlog,
      0,
    );
    const continuousMonitorCandidateTotal = receipts.reduce(
      (sum, receipt) => sum + receipt.prior_continuous_monitor_context_candidates,
      0,
    );
    const gapFreeSeasonSourceWireLaneTotal = receipts.reduce(
      (sum, receipt) => sum + receipt.gap_free_season_source_wire_lane_candidates,
      0,
    );
    const noTransportGapKindTotal = receipts.reduce(
      (sum, receipt) => sum + receipt.no_transport_gap_kind_candidates,
      0,
    );
    if (priorTotal !== Number(proof.summary?.selected_events_with_prior_season_context) ||
      laterTotal !== Number(proof.summary?.selected_events_with_only_later_season_observation) ||
      absentTotal !== Number(proof.summary?.selected_events_without_any_season_observation_in_rlog) ||
      (seasonProofSchema >= 2 && continuousMonitorCandidateTotal !==
        Number(proof.summary?.selected_events_with_prior_continuous_monitor_context_candidate)) ||
      (seasonProofSchema >= 3 && gapFreeSeasonSourceWireLaneTotal !==
        Number(proof.summary?.selected_events_with_gap_free_season_source_wire_lane_candidate)) ||
      (seasonProofSchema >= 3 && noTransportGapKindTotal !==
        Number(proof.summary?.selected_events_with_no_transport_gap_kind_since_candidate)) ||
      proof.summary?.every_selected_event_has_prior_season_context !==
        (priorTotal === events.length && events.length > 0)) {
      throw new Error("Status-event season-context proof aggregate counts are inconsistent");
    }
  }
  return index;
}

function validateSeasonStateMutationProof(proof, proofPath, build) {
  if (!proof && !proofPath) return null;
  const proofSchema = Number(proof?.schema_version);
  if (!proof || !proofPath || ![1, 2].includes(proofSchema) ||
    proof.tool !== "tools/bpsr-season-state-mutation-proof.mjs") {
    throw new Error("Unsupported season-state mutation proof schema or generator");
  }
  requireBuild(proof.game_build, build, "season-state mutation proof");
  if (proof.policy?.exact_input_build_authoritative !== true ||
    proof.policy?.exact_numeric_world_route_authoritative !== true ||
    proof.policy?.complete_direct_literal_lua_writer_scan_required !== true ||
    proof.policy?.dynamically_constructed_field_writers_proven_absent !== false ||
    proof.policy?.native_or_external_state_writers_proven_absent !== false ||
    proof.policy?.monitor_chain_logout_or_clear_absence_proven !== false ||
    proof.policy?.promoted_protocol_event_coverage_proven !== false ||
    proof.policy?.formula_authority !== false || proof.policy?.runtime_authority !== false ||
    proof.policy?.ui_display_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(proof.exact_route?.service_id) !== 1664308034 ||
    Number(proof.exact_route?.method_id) !== 27 ||
    proof.exact_route?.wire_value_field !== "vSeason" ||
    Number(proof.summary?.lua_files_scanned) <= 0 ||
    Number(proof.summary?.candidate_files_decompiled) <= 0 ||
    Number(proof.summary?.decompile_failures) !== 0 ||
    Number(proof.summary?.direct_literal_state_writer_files) !== 2 ||
    Number(proof.summary?.direct_literal_state_writer_assignments) !== 2 ||
    proof.summary?.exact_server_route_to_positive_season_writer_proven !== true ||
    proof.summary?.exact_clear_to_zero_writer_proven !== true ||
    proof.summary?.direct_literal_lua_writer_surface_complete !== true ||
    proof.summary?.event_time_season_authority_complete !== false ||
    proof.direct_literal_writer_surface?.formula_authority !== false ||
    proof.direct_literal_writer_surface?.runtime_authority !== false ||
    proof.direct_literal_writer_surface?.provider_rdps_credit_allowed !== false) {
    throw new Error("Season-state mutation proof is unsafe or incomplete");
  }
  const writers = new Map(
    (proof.direct_literal_writer_surface?.direct_literal_writers ?? []).map((writer) => [
      String(writer?.relative_path),
      (writer?.assignments ?? []).map((assignment) => String(assignment?.expression)),
    ]),
  );
  if (writers.size !== 2 ||
    stableStringify(writers.get("ui/model/season_data.lua")) !== stableStringify(["0"]) ||
    stableStringify(writers.get("ui/view_model/season_vm.lua")) !==
      stableStringify(["seasonId"])) {
    throw new Error("Season-state mutation proof direct writer surface is inconsistent");
  }
  if (proofSchema >= 2) {
    const lifecycle = proof.lifecycle_surface;
    const assertions = lifecycle?.assertions;
    if (proof.policy?.normal_reconnect_static_control_flow_proven !== true ||
      proof.policy?.explicit_logout_reset_static_control_flow_proven !== true ||
      proof.policy?.static_reconnect_lifecycle_never_grants_event_time_logout_exclusion !== true ||
      Number(proof.summary?.required_lifecycle_files_decompiled) !== 7 ||
      Number(proof.summary?.direct_data_manager_clear_callsites) !== 1 ||
      proof.summary?.normal_reconnect_preserves_season_state_by_static_control_flow !== true ||
      proof.summary?.explicit_logout_resets_season_state_by_static_control_flow !== true ||
      proof.summary?.intervening_monitor_chain_explicit_logout_proven_absent !== false ||
      !lifecycle || lifecycle.event_time_authority !== false ||
      lifecycle.formula_authority !== false || lifecycle.runtime_authority !== false ||
      lifecycle.ui_display_authority !== false ||
      lifecycle.provider_rdps_credit_allowed !== false ||
      lifecycle.dynamic_or_aliased_clear_callsites_proven_absent !== false ||
      lifecycle.direct_data_manager_clear_callsites?.length !== 1 ||
      lifecycle.direct_data_manager_clear_callsites[0]?.relative_path !==
        "ui/view_model/login_vm.lua" ||
      lifecycle.data_manager_on_reconnect_callsites?.length !== 1 ||
      lifecycle.data_manager_on_reconnect_callsites[0]?.relative_path !== "game.lua" ||
      assertions?.normal_reconnect_preserves_season_state_by_static_control_flow !== true ||
      assertions?.explicit_logout_resets_season_state_by_static_control_flow !== true ||
      assertions?.intervening_monitor_chain_explicit_logout_proven_absent !== false) {
      throw new Error("Season-state lifecycle proof is unsafe or incomplete");
    }
  }
  return {
    proof: fileDescriptor(proofPath),
    proof_schema_version: proofSchema,
    exact_server_route_to_positive_season_writer_proven: true,
    exact_clear_to_zero_writer_proven: true,
    direct_literal_lua_writer_surface_complete: true,
    dynamic_or_aliased_writers_proven_absent: false,
    intervening_monitor_chain_clear_proven_absent: false,
    normal_reconnect_preserves_season_state_by_static_control_flow:
      proofSchema >= 2,
    explicit_logout_resets_season_state_by_static_control_flow:
      proofSchema >= 2,
    direct_data_manager_clear_callsites: proofSchema >= 2
      ? Number(proof.summary.direct_data_manager_clear_callsites)
      : 0,
    intervening_monitor_chain_explicit_logout_proven_absent: false,
    promoted_protocol_event_coverage_proven: false,
    event_time_season_authority: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validatePartyHasteStackingFrontier(proof, proofPath, partyEffectAuditPath, build) {
  if (!proof && !proofPath) return null;
  if (!proof || !proofPath || ![1, 2].includes(Number(proof.schema_version)) ||
    proof.generated_by !== "tools/bpsr-party-haste-stacking-frontier.mjs") {
    throw new Error("Unsupported party-Haste stacking frontier schema or generator");
  }
  if (Number(proof.schema_version) >= 2 && proof.content_sha256 !== contentHash(proof)) {
    throw new Error("Party-Haste stacking frontier content hash is invalid");
  }
  requireBuild(proof.game_build, build, "party-Haste stacking frontier");
  const auditDescriptor = fileDescriptor(partyEffectAuditPath);
  const summary = proof.summary;
  const surface = proof.observed_lifecycle_surface;
  if (Number(proof.effect_id) !== 31602 ||
    stableStringify(proof.inputs?.party_effect_window_audit) !==
      stableStringify(auditDescriptor) ||
    proof.policy?.only_observed_lifecycle_overlap_is_reported !== true ||
    proof.policy?.static_integer_rule_values_are_not_semantics_without_exact_interpretation !== true ||
    proof.policy?.unknown_stacking_arbitration_is_preserved !== true ||
    proof.policy?.formula_authority !== false || proof.policy?.runtime_authority !== false ||
    proof.policy?.ui_display_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    stableStringify(proof.exact_static_row?.repeat_add_rule) !== stableStringify([1, 1]) ||
    Number(proof.exact_static_row?.time_refresh_type) !== 0 ||
    proof.exact_static_row?.numeric_repeat_add_rule_semantics_proven !== false ||
    proof.exact_static_row?.numeric_time_refresh_type_semantics_proven !== false ||
    proof.exact_static_row?.stacking_arbitration_authority !== false ||
    Number(summary?.status_events) !== 130 || Number(summary?.windows) !== 65 ||
    Number(summary?.windows_with_terminal_event) !== 65 ||
    Number(summary?.orphan_lifecycle_windows) !== 0 ||
    stableStringify(summary?.reported_stack_values) !== stableStringify([1]) ||
    Number(summary?.overlapping_window_pairs) !== 0 ||
    Number(summary?.overlapping_window_pairs_with_distinct_provider_sets) !== 0 ||
    Number(summary?.max_concurrent_windows_for_same_session_and_target) !== 1 ||
    summary?.exact_static_integer_rule_semantics_proven !== false ||
    summary?.server_stacking_arbitration_proven !== false ||
    summary?.downstream_operation_order_and_rounding_proven !== false ||
    summary?.formula_authority !== false || summary?.runtime_authority !== false ||
    summary?.ui_display_authority !== false || summary?.provider_rdps_credit_allowed !== false ||
    Number(surface?.lifecycle_event_count) !== 130 || Number(surface?.windows) !== 65 ||
    surface?.observed_absence_of_overlap_is_not_server_stacking_semantics !== true) {
    throw new Error("Party-Haste stacking frontier is unsafe or inconsistent");
  }
  return {
    proof: fileDescriptor(proofPath),
    exact_party_effect_window_audit_match: true,
    exact_static_repeat_add_rule: [1, 1],
    exact_static_time_refresh_type: 0,
    selected_status_events: 130,
    selected_windows: 65,
    reported_stack_values: [1],
    overlapping_window_pairs: 0,
    distinct_provider_overlap_pairs: 0,
    max_concurrent_windows_for_same_session_and_target: 1,
    exact_static_integer_rule_semantics_proven: false,
    server_stacking_arbitration_proven: false,
    downstream_operation_order_and_rounding_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validatePartyHasteActionFrontier(
  formulaProof,
  formulaPath,
  capacityProof,
  capacityPath,
  ancestryProof,
  ancestryPath,
  build,
) {
  const supplied = [formulaProof, formulaPath, capacityProof, capacityPath, ancestryProof, ancestryPath]
    .filter((value) => value != null).length;
  if (supplied === 0) return null;
  if (supplied !== 6) {
    throw new Error("Party-Haste action frontier requires formula, capacity, and timing proofs together");
  }
  requireBuild(formulaProof.game_build, build, "native action-speed formula proof");
  requireBuild(capacityProof.game_build, build, "party-Haste conditional-capacity proof");
  requireBuild(ancestryProof.game_build, build, "action timing ancestry proof");
  if (Number(formulaProof.schema_version) !== 5 ||
    formulaProof.generated_by !== "tools/bpsr-action-speed-current-build-proof.mjs" ||
    formulaProof.proof_state !==
      "exact-current-build-native-action-speed-float32-operation-order-proven-runtime-join-open" ||
    formulaProof.policy?.exact_numeric_attribute_and_effect_ids_are_authoritative !== true ||
    formulaProof.policy?.missing_or_unobserved_values_are_zero !== false ||
    formulaProof.policy?.remote_player_cast_packets_required !== false ||
    formulaProof.policy?.ordinary_damage_is_retained !== true ||
    formulaProof.policy?.provider_rdps_credit_allowed !== false ||
    formulaProof.summary?.exact_current_build_binary_identity !== true ||
    formulaProof.summary?.exact_current_build_method_identity !== true ||
    formulaProof.summary?.exact_current_build_stage_selection_proven !== true ||
    formulaProof.summary?.exact_skill_scoped_temporary_attribute_lookup_abi_proven !== true ||
    formulaProof.summary?.exact_temporary_attribute_match_operation_and_no_match_zero_proven !== true ||
    formulaProof.summary?.exact_non_singing_algebraic_speed_formulas_proven !== true ||
    formulaProof.summary?.exact_native_float32_operation_order_proven !== true ||
    formulaProof.summary?.exact_action_time_attribute_snapshot_route_proven !== false ||
    formulaProof.summary?.exact_provider_removed_speed_replay_proven !== false ||
    formulaProof.summary?.exact_integer_damage_rounding_proven !== false ||
    formulaProof.summary?.packet_conservation_proven !== false ||
    formulaProof.summary?.runtime_promotion_allowed !== false ||
    Number(formulaProof.summary?.observed_damage_reassigned_to_provider) !== 0) {
    throw new Error("Native action-speed formula proof is unsafe or incomplete");
  }
  if (Number(capacityProof.schema_version) !== 5 ||
    capacityProof.generated_by !== "tools/bpsr-action-speed-provider-removed-capacity-proof.mjs" ||
    Number(capacityProof.effect_id) !== 31602 ||
    capacityProof.proof_state !==
      "exact-damage-time-and-recipient-coefficient-conditional-capacity-proven-action-opportunity-open" ||
    !sameFileContentIdentity(
      capacityProof.inputs?.native_action_speed_formula,
      fileDescriptor(formulaPath),
    ) ||
    capacityProof.policy?.damage_event_time_is_action_start_time !== false ||
    capacityProof.policy?.missing_remote_cast_packets_required !== false ||
    capacityProof.policy?.hypothetical_capacity_is_observed_damage !== false ||
    capacityProof.policy?.hypothetical_capacity_may_be_reassigned_to_provider !== false ||
    capacityProof.policy?.ordinary_damage_totals_unchanged !== true ||
    capacityProof.policy?.provider_rdps_credit_allowed !== false ||
    capacityProof.policy?.ui_rdps_display_allowed !== false ||
    Number(capacityProof.summary?.responsive_damage_action_memberships) !== 3713 ||
    String(capacityProof.summary?.responsive_reported_damage_units) !== "556129992" ||
    Number(capacityProof.summary?.conditional_capacity_groups) !== 54 ||
    Number(capacityProof.summary?.conditional_capacity_memberships) !== 3185 ||
    String(capacityProof.summary?.conditional_capacity_reported_damage_units) !== "350049695" ||
    Number(capacityProof.summary?.proven_self_provider_exclusion_memberships) !== 528 ||
    String(capacityProof.summary?.proven_self_provider_exclusion_reported_damage_units) !==
      "206080297" ||
    capacityProof.summary?.self_provider_damage_stays_with_damage_actor !== true ||
    Number(capacityProof.summary?.unresolved_recipient_mode_memberships) !== 0 ||
    capacityProof.summary?.exact_damage_time_speed_state_proven !== true ||
    capacityProof.summary?.reversible_recipient_coefficient_join_proven_for_conditional_rows !== true ||
    capacityProof.summary?.exact_native_speed_float32_operation_order_proven !== true ||
    capacityProof.summary?.exact_membership_speed_stage_route_proven !== true ||
    capacityProof.summary?.rationalized_conditional_capacity_is_exact_native_float32_replay !== false ||
    capacityProof.summary?.runtime_temporary_speed_term_zero_allowed !== false ||
    capacityProof.summary?.exact_action_opportunity_proven !== false ||
    capacityProof.summary?.integer_rounding_proven !== false ||
    capacityProof.summary?.packet_conservation_proven !== false ||
    capacityProof.summary?.ui_rdps_display_allowed !== false ||
    capacityProof.summary?.provider_rdps_credit_allowed !== false ||
    capacityProof.summary?.runtime_promotion_allowed !== false ||
    Number(capacityProof.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    Number(capacityProof.formula?.fixed_point_scale) !== 10000 ||
    Number(capacityProof.formula?.temporary_term_assumed_by_conditional_calculation) !== 0 ||
    capacityProof.formula?.rationalized_capacity_is_exact_native_float32_replay !== false ||
    capacityProof.content_sha256 !== orderedContentHash(capacityProof)) {
    throw new Error("Party-Haste conditional-capacity proof is unsafe or incomplete");
  }
  if (Number(ancestryProof.schema_version) !== 13 ||
    ancestryProof.generated_by !== "tools/bpsr-action-hit-timing-ancestry-proof.mjs" ||
    Number(ancestryProof.effect_id) !== 31602 ||
    ancestryProof.proof_state !==
      "native-parser-and-scheduler-formulas-proven-catalog-key-mapping-observed-speed-value-motion-and-packet-clock-joins-open" ||
    !sameFileContentIdentity(
      ancestryProof.inputs?.damage_action_membership_ledger,
      capacityProof.inputs?.source_side_damage_action_membership_ledger,
    ) ||
    ancestryProof.policy?.remote_player_cast_packets_required !== false ||
    ancestryProof.policy?.missing_timing_parameters_are_zero !== false ||
    ancestryProof.policy?.packet_damage_group_is_remote_action_instance !== false ||
    ancestryProof.policy?.interpolated_game_time_is_packet_damage_action_timestamp !== false ||
    ancestryProof.policy?.ordinary_damage_totals_unchanged !== true ||
    ancestryProof.policy?.provider_rdps_credit_allowed !== false ||
    ancestryProof.policy?.ui_rdps_display_allowed !== false ||
    Number(ancestryProof.summary?.responsive_damage_event_join_groups) !== 46 ||
    Number(ancestryProof.summary?.responsive_damage_event_join_memberships) !== 3713 ||
    Number(ancestryProof.summary?.exact_one_damage_event_match_groups) !== 40 ||
    Number(ancestryProof.summary?.exact_one_damage_event_match_memberships) !== 3466 ||
    Number(ancestryProof.summary?.unresolved_no_damage_event_groups) !== 6 ||
    Number(ancestryProof.summary?.unresolved_no_damage_event_memberships) !== 247 ||
    ancestryProof.summary?.standard_hit_event_to_parser_route_proven !== true ||
    ancestryProof.summary?.standard_hitdata_native_timing_formula_proven !== true ||
    ancestryProof.summary?.native_scheduler_speed_scaling_formula_proven !== true ||
    ancestryProof.summary?.action_speed_formula_to_scheduler_mechanism_route_proven !== true ||
    ancestryProof.summary?.action_speed_formula_native_sampling_point_proven !== true ||
    ancestryProof.summary?.parser_lookup_global_to_catalog_parameter_identity_proven !== false ||
    Number(ancestryProof.summary?.exact_scheduler_speed_value_join_memberships) !== 0 ||
    ancestryProof.summary?.effective_speed_scaled_timing_materialized !== false ||
    ancestryProof.summary?.begin_time_unit_proven !== false ||
    ancestryProof.summary?.repetition_schedule_proven !== false ||
    ancestryProof.summary?.provider_rdps_credit_allowed !== false ||
    ancestryProof.summary?.ui_rdps_display_allowed !== false ||
    ancestryProof.summary?.runtime_promotion_allowed !== false ||
    Number(ancestryProof.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    ancestryProof.content_sha256 !== orderedContentHash(ancestryProof)) {
    throw new Error("Action timing ancestry proof is unsafe or incomplete");
  }
  return {
    native_action_speed_formula_proof: fileDescriptor(formulaPath),
    conditional_capacity_proof: fileDescriptor(capacityPath),
    action_timing_ancestry_proof: fileDescriptor(ancestryPath),
    exact_native_float32_operation_order_proven: true,
    exact_non_singing_algebraic_speed_formulas_proven: true,
    exact_temporary_attribute_lookup_abi_proven: true,
    exact_action_speed_native_sampling_point_proven: true,
    exact_scheduler_speed_scaling_formula_proven: true,
    responsive_damage_action_memberships: 3713,
    responsive_reported_damage_units: "556129992",
    conditional_capacity_groups: 54,
    conditional_capacity_memberships: 3185,
    conditional_capacity_reported_damage_units: "350049695",
    self_provider_exclusion_memberships: 528,
    exact_one_damage_event_match_memberships: 3466,
    unresolved_no_damage_event_memberships: 247,
    conditional_capacity_formula: String(capacityProof.formula.conditional_capacity_damage),
    fixed_point_scale: 10000,
    bit_equivalent_float32_input_snapshot_replay_proven: false,
    exact_action_opportunity_proven: false,
    packet_clock_correspondence_proven: false,
    integer_damage_rounding_proven: false,
    packet_conservation_proven: false,
    observed_damage_reassigned_to_provider: 0,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validateTargetVulnerabilityFormulaProof(proof, proofPath, build) {
  if (!proof && !proofPath) return null;
  const proofSchema = Number(proof?.schema_version);
  if (!proof || !proofPath || ![4, 13, 14, 15, 16, 17, 18, 19, 20, 21].includes(proofSchema) ||
    proof.generated_by !== "tools/bpsr-target-vulnerability-formula-proof.mjs") {
    throw new Error("Unsupported target-vulnerability formula proof schema or generator");
  }
  requireBuild(proof.game_build, build, "target-vulnerability formula proof");
  if (Number(proof.effect_id) !== 55228 ||
    proof.relationship_model?.effect_endpoint_and_damage_target_are_independent !== true ||
    proof.relationship_model?.endpoint_allegiance_assumed !== false ||
    proof.relationship_model?.remote_player_cast_packets_required !== false ||
    proof.historical_candidate?.build_identity_matches !== false ||
    proof.historical_candidate?.current_build_formula_authority !== false ||
    proof.candidate_equation?.exact_current_build_scalar_proven !== false ||
    proof.candidate_equation?.exact_current_build_operation_order_proven !== false ||
    proof.promotion_gate?.missing_required_input !== null ||
    stableStringify(proof.promotion_gate?.required_proof_suites) !==
      stableStringify(["canonical-replay-conservation"]) ||
    proof.promotion_gate?.ready_for_snapshot !== true ||
    proof.promotion_gate?.protocol_pack_promotion_allowed !== true ||
    proof.promotion_gate?.protocol_event_coverage_proven !== true ||
    proof.promotion_gate?.exact_pack_gap_free_segment_ordinary_damage_conservation_proven !== true ||
    proof.promotion_gate?.exact_pack_closed_lifecycle_canonical_replay_conservation_proven !== false ||
    proof.promotion_gate?.formula_specific_counterfactual_conservation_proven !== false ||
    proof.promotion_gate?.runtime_promotion_allowed !== false ||
    proof.summary?.catalog_review_state !== "candidate" ||
    proof.summary?.current_build_magnitude_proven !== false ||
    proof.summary?.formula_authority !== false || proof.summary?.runtime_authority !== false ||
    proof.summary?.ui_display_authority !== false ||
    proof.summary?.provider_rdps_credit_allowed !== false ||
    stableStringify(proof.summary?.production_rule_effect_ids) !== stableStringify([]) ||
    stableStringify(proof.summary?.offline_candidate_effect_ids) !== stableStringify([55228]) ||
    proof.summary?.unresolved_evidence_hidden !== false ||
    Number(proof.current_build_evidence?.provider_owned_status_events) !== 13842 ||
    Number(proof.current_build_evidence?.target_active_damage_samples) !== 269731 ||
    Number(proof.current_build_evidence?.action_counterfactual_samples) !== 56083 ||
    Number(proof.current_build_evidence?.provider_complete_present_absent_controlled_groups) !== 0 ||
    Number(proof.current_build_evidence?.corrected_controlled_present_absent_groups) !== 4 ||
    Number(proof.current_build_evidence?.corrected_controlled_sample_comparisons) !== 7 ||
    proof.current_build_evidence?.controlled_observed_damage_delta_proven !== true ||
    proof.current_build_evidence?.historical_plus_1000_candidate_compatible !== true ||
    proof.current_build_evidence?.historical_plus_1000_candidate_unique !== false ||
    stableStringify(proof.current_build_evidence?.compatible_total_factor_basis_points) !==
      stableStringify([{ minimum: 18455, maximum: 18456 }]) ||
    Number(proof.current_build_evidence?.candidate_replay_events) !== 425 ||
    String(proof.current_build_evidence?.candidate_replay_delta) !== "20300571" ||
    proof.current_build_evidence?.candidate_replay_conserved !== true ||
    Number(proof.current_build_evidence?.deferred_multiple_provider_windows) !== 215 ||
    Number(proof.current_build_evidence?.decoded_table_files_scanned) !== 577 ||
    Number(proof.current_build_evidence?.decoded_table_bytes_scanned) !== 172314811 ||
    Number(proof.current_build_evidence?.decoded_exact_id_occurrences_retained) !== 1242 ||
    Number(proof.source_chain_boundary?.provider_skill_id) !== 2209 ||
    Number(proof.source_chain_boundary?.provider_skill_effect_id) !== 220901 ||
    Number(proof.source_chain_boundary?.status_id) !== 55228 ||
    proof.source_chain_boundary?.exact_skill_to_skill_effect_edge_proven !== true ||
    proof.source_chain_boundary?.reviewed_skill_to_status_candidate_preserved !== true ||
    proof.source_chain_boundary?.exact_skill_to_status_edge_proven !== false ||
    proof.source_chain_boundary?.exact_typed_skill_or_skill_effect_reference_to_status_found !== false ||
    proof.source_chain_boundary?.status_exact_id_linked_scalar_candidate_found !== false ||
    proof.source_chain_boundary?.indirect_or_server_formula_ruled_out !== false ||
    Number(proof.source_chain_boundary?.recipient_damage_measurement_action_id) !== 2203291 ||
    proof.source_chain_boundary?.recipient_damage_measurement_action_is_provider_skill_identity !== false ||
    (proofSchema >= 13 &&
      (proof.action_near_pair_diagnostic
        ?.source_statuses_proven_neutral_to_selected_parent_hit_amount !== true ||
       proof.action_near_pair_diagnostic?.pair_rejected_by_trigger_ancestry_adjudication !== false ||
       Number(proof.action_near_pair_diagnostic?.trigger_ancestry_effect_id) !== 2203182 ||
       Number(proof.action_near_pair_diagnostic?.selected_action_damage_attr_id) !== 2220329109 ||
       Number(proof.action_near_pair_diagnostic
         ?.selected_action_damage_coefficient_basis_points) !== 35000 ||
       proof.action_near_pair_diagnostic?.observed_current_build_parent_hit_delta_proven !== true ||
       Number(proof.action_near_pair_diagnostic?.observed_current_build_parent_hit_delta) !== 6626 ||
       proof.action_near_pair_diagnostic?.exact_scalar_proven !== false)) ||
    (proofSchema === 14 &&
      (proof.selected_pair_factor_frontier?.exact_action_factor_target_edges_proven !== true ||
       proof.selected_pair_factor_frontier?.encounter_factor_grades_proven !== false ||
       proof.selected_pair_factor_frontier
         ?.catalog_only_factor_item_links_are_runtime_authority !== false ||
       proof.selected_pair_factor_frontier
         ?.phantom_falcon_2000_basis_points_excluded_for_selected_action !== true ||
       stableStringify(proof.selected_pair_factor_frontier
         ?.diagnostic_compatible_total_factor_basis_points) !== stableStringify([19801]) ||
       Number(proof.selected_pair_factor_frontier
         ?.diagnostic_compatible_component_decompositions) !== 4 ||
       proof.selected_pair_factor_frontier?.unique_component_decomposition_proven !== false ||
       proof.selected_pair_factor_frontier?.exact_scalar_proven !== false ||
       proof.selected_pair_factor_frontier?.runtime_promotion_allowed !== false)) ||
    (proofSchema === 15 &&
      (proof.selected_pair_factor_frontier?.exact_action_factor_target_edges_proven !== true ||
       proof.selected_pair_factor_frontier?.encounter_factor_grades_proven !== true ||
       proof.selected_pair_factor_frontier
         ?.duplicate_marksman_x10_selection_across_active_lines_observed !== true ||
       proof.selected_pair_factor_frontier
         ?.encounter_factor_effective_combination_proven !== false ||
       proof.selected_pair_factor_frontier
         ?.catalog_only_factor_item_links_are_runtime_authority !== false ||
       proof.selected_pair_factor_frontier
         ?.phantom_falcon_2000_basis_points_excluded_for_selected_action !== true ||
       Number(proof.selected_pair_factor_frontier
         ?.observed_selection_additive_model_count) !== 3 ||
       Number(proof.selected_pair_factor_frontier
         ?.observed_selection_compatible_additive_decompositions) !== 0 ||
       stableStringify(proof.selected_pair_factor_frontier
         ?.unobserved_grade_diagnostic_compatible_total_factor_basis_points) !==
           stableStringify([19801]) ||
       Number(proof.selected_pair_factor_frontier
         ?.unobserved_grade_diagnostic_compatible_component_decompositions) !== 4 ||
       proof.selected_pair_factor_frontier
         ?.unobserved_grade_diagnostic_contradicted_by_same_capture_profile !== true ||
       proof.selected_pair_factor_frontier?.unique_component_decomposition_proven !== false ||
       proof.selected_pair_factor_frontier?.exact_scalar_proven !== false ||
       proof.selected_pair_factor_frontier?.runtime_promotion_allowed !== false)) ||
    (proofSchema >= 16 &&
      (proof.selected_pair_factor_frontier?.exact_action_factor_target_edges_proven !== true ||
       proof.selected_pair_factor_frontier?.encounter_factor_grades_proven !== true ||
       proof.selected_pair_factor_frontier
         ?.duplicate_marksman_x10_selection_across_active_lines_observed !== true ||
       proof.selected_pair_factor_frontier
         ?.encounter_factor_effective_combination_proven !== true ||
       Number(proof.selected_pair_factor_frontier?.selected_pair_scene_id) !== 6525 ||
       Number(proof.selected_pair_factor_frontier?.selected_pair_dungeon_play_type) !== 17 ||
       Number(proof.selected_pair_factor_frontier
         ?.selected_pair_effective_factor_instances?.length) !== 2 ||
       Number(proof.selected_pair_factor_frontier
         ?.selected_pair_excluded_factor_instances?.length) !== 1 ||
       proof.selected_pair_factor_frontier
         ?.catalog_only_factor_item_links_are_runtime_authority !== false ||
       proof.selected_pair_factor_frontier
         ?.phantom_falcon_2000_basis_points_excluded_for_selected_action !== true ||
       Number(proof.selected_pair_factor_frontier
         ?.observed_selection_additive_model_count) !== 3 ||
       Number(proof.selected_pair_factor_frontier
         ?.observed_selection_compatible_additive_decompositions) !== 0 ||
       stableStringify(proof.selected_pair_factor_frontier
         ?.unobserved_grade_diagnostic_compatible_total_factor_basis_points) !==
           stableStringify([19801]) ||
       Number(proof.selected_pair_factor_frontier
         ?.unobserved_grade_diagnostic_compatible_component_decompositions) !== 4 ||
       proof.selected_pair_factor_frontier
         ?.unobserved_grade_diagnostic_contradicted_by_same_capture_profile !== true ||
       Number(proof.selected_pair_factor_frontier?.module_2104_context?.module_effect_id) !== 2104 ||
       Number(proof.selected_pair_factor_frontier?.module_2104_context?.selected_level) !== 6 ||
       Number(proof.selected_pair_factor_frontier?.module_2104_context
         ?.selected_configured_raw_damage_increase) !== 1100 ||
       Number(proof.selected_pair_factor_frontier?.module_2104_context
         ?.nonmatching_provider_instances_excluded) !== 2 ||
       Number(proof.selected_pair_factor_frontier?.module_2104_context
         ?.current_build_runtime_ladders) !== 31 ||
       proof.selected_pair_factor_frontier?.module_2104_context
         ?.baseline_zone_raw_1000_proven !== false ||
       proof.selected_pair_factor_frontier?.module_2104_context
         ?.integer_rounding_model_unique !== false ||
       proof.selected_pair_factor_frontier?.module_2104_context
         ?.transferable_provider_rdps_component !== false ||
       Number(proof.selected_pair_factor_frontier
         ?.observed_selection_with_module_same_bucket_compatible_additive_decompositions) !== 0 ||
       Number(proof.selected_pair_factor_frontier
         ?.unobserved_grade_with_module_required_compatible_component_decompositions) !== 6 ||
       proof.selected_pair_factor_frontier
         ?.observed_encounter_grade_10_with_module_same_bucket_compatible !== false ||
       (proofSchema >= 17 &&
         (Number(proof.selected_pair_factor_frontier
           ?.physical_boost_12550_context?.attribute_id) !== 12550 ||
          Number(proof.selected_pair_factor_frontier
            ?.physical_boost_12550_context?.raw_value) !== 600 ||
          Number(proof.selected_pair_factor_frontier
            ?.physical_boost_12550_context?.display_percent) !== 6 ||
          proof.selected_pair_factor_frontier
            ?.physical_boost_12550_context?.same_state_id_in_present_and_absent_samples !== true ||
          proof.selected_pair_factor_frontier
            ?.physical_boost_12550_context?.exact_formula_stage_proven !== false ||
          proof.selected_pair_factor_frontier
            ?.physical_boost_12550_context
            ?.same_target_vulnerability_bucket_as_effect_55228_proven !== false ||
          Number(proof.selected_pair_factor_frontier?.rorola_3948_context?.skill_id) !== 3948 ||
          Number(proof.selected_pair_factor_frontier?.rorola_3948_context?.base_effect_id) !== 2110111 ||
          Number(proof.selected_pair_factor_frontier?.rorola_3948_context?.timer_effect_id) !== 2110135 ||
          Number(proof.selected_pair_factor_frontier?.rorola_3948_context?.counter_effect_id) !== 2110136 ||
          Number(proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.selected_remodel_level) !== 5 ||
          Number(proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.personal_base_damage_boost_raw) !== 1000 ||
          Number(proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.personal_additional_damage_boost_raw_per_ten_hits) !== 120 ||
          Number(proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.source_counter_stacks) !== 11 ||
          Number(proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.diagnostic_personal_raw_value) !== 1120 ||
          String(proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.excluded_external_provider_entity_uuid) !== "190072160896" ||
          proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.provider_scoped_lifecycle_proven !== true ||
          proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.transferable_provider_rdps_component !== false ||
          proof.selected_pair_factor_frontier
            ?.rorola_3948_context?.exact_formula_bucket_placement_proven !== false ||
          Number(proof.selected_pair_factor_frontier
            ?.observed_selection_with_rorola_same_bucket_compatible_additive_decompositions) !== 0 ||
          Number(proof.selected_pair_factor_frontier
            ?.observed_selection_with_module_and_rorola_same_bucket_compatible_additive_decompositions) !== 0)) ||
       (proofSchema >= 18 &&
         (Number(proof.selected_pair_factor_frontier
           ?.target_attribute_control_context?.only_changed_attribute_id) !== 11310 ||
          proof.selected_pair_factor_frontier
            ?.target_attribute_control_context?.only_changed_attribute_enum_name !== "AttrHp" ||
          proof.selected_pair_factor_frontier
            ?.target_attribute_control_context?.only_changed_attribute_is_current_hp !== true ||
          proof.selected_pair_factor_frontier
            ?.target_attribute_control_context?.all_other_retained_target_attributes_equal !== true ||
          proof.selected_pair_factor_frontier
            ?.target_attribute_control_context?.exact_selected_action_current_hp_independence_proven !== false ||
          Number(proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.shared_status_instances) !== 120 ||
          Number(proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.mapped_status_instances) !== 75 ||
          Number(proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.unmapped_status_instances) !== 45 ||
          Number(proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.unmapped_distinct_effect_ids?.length) !== 43 ||
          Number(proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.damage_modifier_manifestations) !== 11 ||
          Number(proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.distinct_damage_modifier_effect_ids?.length) !== 10 ||
          proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.all_shared_status_instances_mapped !== false ||
          proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.complete_static_inventory_proves_runtime_bucket_membership !== false ||
          proof.selected_pair_factor_frontier
            ?.active_modifier_inventory?.complete_static_inventory_proves_server_current_hp_independence !== false)) ||
       (proofSchema >= 19 &&
         (Number(proof.selected_pair_factor_frontier?.active_modifier_inventory
           ?.formerly_unmapped_triage?.distinct_effect_ids) !== 43 ||
          Number(proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.current_build_classification_entries) !== 24 ||
          Number(proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.current_build_formula_term_entries) !== 32 ||
          Number(proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.current_build_value_proof_entries) !== 10 ||
          Number(proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.static_damage_or_stat_formula_zone_candidates) !== 15 ||
          Number(proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.no_static_route_entries) !== 11 ||
          Number(proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.target_locus_semantic_current_hp_candidates) !== 0 ||
          proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.static_triage_is_runtime_formula_authority !== false ||
          proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.exact_server_current_hp_independence_proven !== false ||
          proof.selected_pair_factor_frontier?.active_modifier_inventory
            ?.formerly_unmapped_triage?.provider_rdps_credit_allowed !== false)) ||
       (proofSchema >= 20 &&
         (Number(proof.selected_pair_factor_frontier?.target_attribute_control_context
           ?.semantic_current_hp_target_locus_candidates) !== 0 ||
          Number(proof.selected_pair_factor_frontier?.target_attribute_control_context
            ?.active_state_dependent_direct_source_intersections) !== 1 ||
          Number(proof.selected_pair_factor_frontier?.target_attribute_control_context
            ?.defensive_owner_routes_excluded_from_selected_outgoing_damage) !== 1 ||
          Number(proof.selected_pair_factor_frontier?.target_attribute_control_context
            ?.selected_outgoing_health_dependent_catalog_routes_remaining) !== 0 ||
          proof.selected_pair_factor_frontier?.target_attribute_control_context
            ?.intrinsic_server_action_target_current_hp_behavior_still_open !== true)) ||
       (proofSchema >= 21 &&
         (Number(proof.controlled_baseline_component_frontier?.controlled_groups) !== 4 ||
          Number(proof.controlled_baseline_component_frontier
            ?.bounded_component_scan_samples) !== 56083 ||
          stableStringify(proof.controlled_baseline_component_frontier?.target_monster_ids) !==
            stableStringify([33527, 33529, 33530]) ||
          proof.controlled_baseline_component_frontier
            ?.target_monsters_all_normal_rank !== true ||
          proof.controlled_baseline_component_frontier
            ?.cuisine_elite_damage_clause_applies !== false ||
          proof.controlled_baseline_component_frontier
            ?.third_party_module_transition_found !== true ||
          String(proof.controlled_baseline_component_frontier
            ?.third_party_module_provider_entity_uuid) !== "190072160896" ||
          Number(proof.controlled_baseline_component_frontier
            ?.third_party_module_observed_delta) !== 15844 ||
          proof.controlled_baseline_component_frontier
            ?.third_party_module_external_transfer_proven !== false ||
          proof.controlled_baseline_component_frontier
            ?.critical_output_invariant_across_three_target_hp_states !== true ||
          proof.controlled_baseline_component_frontier
            ?.intrinsic_server_action_target_hp_behavior_globally_excluded !== false ||
          proof.controlled_baseline_component_frontier?.exact_scalar_proven !== false ||
          proof.controlled_baseline_component_frontier?.formula_authority !== false ||
          proof.controlled_baseline_component_frontier?.runtime_promotion_allowed !== false)) ||
       proof.selected_pair_factor_frontier?.unique_component_decomposition_proven !== false ||
       proof.selected_pair_factor_frontier?.exact_scalar_proven !== false ||
       proof.selected_pair_factor_frontier?.runtime_promotion_allowed !== false)) ||
    proof.content_sha256 !== `sha256:${contentHash(proof)}`) {
    throw new Error("Target-vulnerability formula proof is unsafe or incomplete");
  }
  return {
    input: fileDescriptor(proofPath),
    effect_id: 55228,
    proof_state:
      proofSchema >= 21
        ? "current-build-provider-lifecycle-controlled-baseline-components-and-target-rank-narrowed-third-party-module-and-intrinsic-current-hp-open-exact-scalar-open"
        : proofSchema >= 20
        ? "current-build-provider-lifecycle-action-edges-encounter-factor-and-complete-active-status-inventory-catalog-health-routes-narrowed-intrinsic-current-hp-and-exact-scalar-open"
        : proofSchema >= 19
        ? "current-build-provider-lifecycle-action-edges-encounter-factor-and-complete-active-status-inventory-current-hp-and-static-unmapped-triage-gaps-open-exact-scalar-open"
        : proofSchema >= 18
        ? "current-build-provider-lifecycle-action-edges-encounter-factor-and-complete-active-status-inventory-current-hp-and-unmapped-status-gaps-open-exact-scalar-open"
        : proofSchema >= 17
        ? "current-build-provider-lifecycle-action-edges-encounter-factor-selection-effective-combination-module-phy-boost-and-rorola-context-and-candidate-replay-conservation-proven-exact-scalar-open"
        : proofSchema >= 16
        ? "current-build-provider-lifecycle-action-edges-encounter-factor-selection-effective-combination-module-context-and-candidate-replay-conservation-proven-exact-scalar-open"
        : proofSchema >= 15
        ? "current-build-provider-lifecycle-action-edges-encounter-factor-selection-and-candidate-replay-conservation-proven-effective-combination-and-exact-scalar-open"
        : "current-build-provider-lifecycle-action-edges-and-candidate-replay-conservation-proven-exact-scalar-open",
    provider_owned_status_events: 13842,
    target_active_damage_samples: 269731,
    action_counterfactual_samples: 56083,
    provider_complete_present_absent_controlled_groups: 0,
    corrected_controlled_present_absent_groups: 4,
    corrected_controlled_sample_comparisons: 7,
    controlled_observed_damage_delta_proven: true,
    historical_plus_1000_candidate_compatible: true,
    historical_plus_1000_candidate_unique: false,
    compatible_total_factor_basis_points: [{ minimum: 18455, maximum: 18456 }],
    damage_attr_id: Number(proof.current_build_evidence.damage_attr_id),
    candidate_ability_id: Number(proof.candidate_equation.ability_id),
    candidate_replay_events: 425,
    candidate_replay_delta: "20300571",
    candidate_replay_conserved: true,
    candidate_replay_relationship_groups:
      Number(proof.current_build_evidence.candidate_replay_relationship_groups),
    deferred_multiple_provider_windows: 215,
    decoded_table_files_scanned: 577,
    decoded_table_bytes_scanned: 172314811,
    decoded_exact_id_occurrences_retained: 1242,
    provider_skill_id: 2209,
    provider_skill_effect_id: 220901,
    exact_skill_to_skill_effect_edge_proven: true,
    reviewed_skill_to_status_candidate_preserved: true,
    exact_skill_to_status_edge_proven: false,
    exact_typed_skill_or_skill_effect_reference_to_status_found: false,
    status_exact_id_linked_scalar_candidate_found: false,
    indirect_or_server_formula_ruled_out: false,
    recipient_damage_measurement_action_is_provider_skill_identity: false,
    encounter_factor_grades_proven: proofSchema >= 15,
    duplicate_marksman_x10_selection_across_active_lines_observed: proofSchema >= 15,
    encounter_factor_effective_combination_proven: proofSchema >= 16,
    observed_selection_compatible_additive_decompositions: proofSchema >= 15 ? 0 : null,
    module_2104_level_and_scalar_proven: proofSchema >= 16,
    module_2104_selected_matching_provider_stacks: proofSchema >= 16 ? 4 : null,
    module_2104_selected_configured_raw_damage_increase: proofSchema >= 16 ? 1100 : null,
    module_2104_nonmatching_provider_instances_excluded: proofSchema >= 16 ? 2 : null,
    module_2104_transferable_provider_rdps_component: false,
    module_2104_baseline_zone_raw_1000_proven: false,
    module_2104_integer_rounding_model_unique: false,
    physical_boost_12550_identity_and_selected_raw_value_proven: proofSchema >= 17,
    physical_boost_12550_selected_raw_value: proofSchema >= 17 ? 600 : null,
    physical_boost_12550_same_state_in_present_and_absent_samples: proofSchema >= 17,
    physical_boost_12550_exact_formula_stage_proven: false,
    physical_boost_12550_same_target_vulnerability_bucket_as_effect_55228_proven: false,
    rorola_3948_selected_profile_and_personal_values_proven: proofSchema >= 17,
    rorola_3948_selected_counter_stacks: proofSchema >= 17 ? 11 : null,
    rorola_3948_diagnostic_personal_raw_value: proofSchema >= 17 ? 1120 : null,
    rorola_3948_external_provider_copy_excluded: proofSchema >= 17,
    rorola_3948_transferable_provider_rdps_component: false,
    rorola_3948_exact_formula_bucket_placement_proven: false,
    selected_pair_rorola_same_bucket_compatible_decompositions: proofSchema >= 17 ? 0 : null,
    selected_pair_module_and_rorola_same_bucket_compatible_decompositions:
      proofSchema >= 17 ? 0 : null,
    selected_pair_only_target_attribute_difference_is_current_hp: proofSchema >= 18,
    selected_pair_current_hp_delta_present_minus_absent: proofSchema >= 18 ? 1312178 : null,
    selected_pair_exact_action_current_hp_independence_proven: false,
    selected_pair_shared_status_instances: proofSchema >= 18 ? 120 : null,
    selected_pair_mapped_status_instances: proofSchema >= 18 ? 75 : null,
    selected_pair_unmapped_status_instances: proofSchema >= 18 ? 45 : null,
    selected_pair_unmapped_distinct_effect_ids: proofSchema >= 18 ? 43 : null,
    selected_pair_mapped_damage_modifier_manifestations: proofSchema >= 18 ? 11 : null,
    selected_pair_unmapped_current_build_classification_entries: proofSchema >= 19 ? 24 : null,
    selected_pair_unmapped_current_build_formula_term_entries: proofSchema >= 19 ? 32 : null,
    selected_pair_unmapped_current_build_value_proof_entries: proofSchema >= 19 ? 10 : null,
    selected_pair_unmapped_static_damage_or_stat_formula_zone_candidates:
      proofSchema >= 19 ? 15 : null,
    selected_pair_unmapped_no_static_route_entries: proofSchema >= 19 ? 11 : null,
    selected_pair_unmapped_target_locus_semantic_current_hp_candidates:
      proofSchema >= 19 ? 0 : null,
    selected_pair_unmapped_static_triage_is_runtime_formula_authority: false,
    selected_pair_semantic_current_hp_target_locus_candidates: proofSchema >= 20 ? 0 : null,
    selected_pair_active_state_dependent_direct_source_intersections:
      proofSchema >= 20 ? 1 : null,
    selected_pair_defensive_owner_routes_excluded_from_selected_outgoing_damage:
      proofSchema >= 20 ? 1 : null,
    selected_pair_outgoing_health_dependent_catalog_routes_remaining:
      proofSchema >= 20 ? 0 : null,
    selected_pair_intrinsic_server_action_target_current_hp_behavior_still_open:
      proofSchema >= 20,
    controlled_baseline_component_scan_proven: proofSchema >= 21,
    controlled_baseline_target_monsters_all_normal_rank: proofSchema >= 21,
    controlled_baseline_cuisine_elite_damage_clause_excluded: proofSchema >= 21,
    controlled_baseline_third_party_module_transition_found: proofSchema >= 21,
    controlled_baseline_third_party_module_provider_entity_uuid:
      proofSchema >= 21 ? "190072160896" : null,
    controlled_baseline_third_party_module_observed_delta: proofSchema >= 21 ? 15844 : null,
    controlled_baseline_third_party_module_external_transfer_proven: false,
    controlled_baseline_critical_output_invariant_across_three_target_hp_states:
      proofSchema >= 21,
    controlled_baseline_intrinsic_server_action_target_hp_behavior_globally_excluded: false,
    selected_pair_all_shared_status_instances_mapped: false,
    selected_pair_complete_static_inventory_proves_runtime_bucket_membership: false,
    selected_pair_complete_static_inventory_proves_server_current_hp_independence: false,
    historical_observation_build: String(proof.historical_candidate.observation_build),
    historical_build_matches_current_build: false,
    exact_current_build_scalar_proven: false,
    exact_current_build_operation_order_proven: false,
    stacking_and_multi_provider_split_proven: false,
    integer_rounding_proven: false,
    protocol_pack_promotion_allowed: true,
    protocol_event_coverage_proven: true,
    exact_pack_gap_free_segment_ordinary_damage_conservation_proven: true,
    exact_pack_closed_lifecycle_canonical_replay_conservation_proven: false,
    formula_specific_counterfactual_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
    observed_damage_reassigned_to_provider: 0,
  };
}

function attachTargetVulnerabilityFormulaReceipt(frontier, receipt) {
  if (!receipt) return;
  const effect = frontier.effect_results.find((entry) => Number(entry.effect_id) === 55228);
  if (!effect) throw new Error("Target-vulnerability receipt has no party-effect frontier row");
  if (Number(effect.status_events) !== Number(receipt.provider_owned_status_events) ||
    effect.provider_ownership_proven_for_every_status_event !== true) {
    throw new Error("Target-vulnerability receipt does not match provider-owned lifecycle evidence");
  }
  effect.target_vulnerability_formula = structuredClone(receipt);
  effect.blockers = uniqueSorted([
    ...effect.blockers,
    "exact-current-build-target-vulnerability-scalar-and-operation-order-open",
    "multi-provider-stacking-and-split-open",
    "canonical-replay-and-formula-specific-conservation-open",
    "exact-party-skill-to-status-source-edge-open",
    ...(receipt.encounter_factor_effective_combination_proven
      ? []
      : ["encounter-factor-effective-combination-open"]),
  ]);
}

function validateImagineFormulaProof(proof, proofPath, build) {
  const index = new Map();
  if (!proof && !proofPath) return index;
  if (!proof || !proofPath || Number(proof.schema_version) !== 1 ||
    proof.generated_by !== "tools/rdps-imagine-formula-proof.mjs") {
    throw new Error("Unsupported Imagine formula proof schema or generator");
  }
  requireBuild(proof.game_build, build, "Imagine formula proof");
  if (proof.policy?.exact_current_build_only !== true ||
    proof.policy?.run_owned_equipped_imagine_identity_and_tier_are_authoritative !== true ||
    proof.policy?.later_profile_snapshots_never_rewrite_historical_runs !== true ||
    proof.policy?.direct_summon_damage_remains_owner_damage !== true ||
    proof.policy?.static_scalar_or_native_equation_does_not_enable_rdps_without_event_replay !== true ||
    proof.policy?.external_lifecycle_is_required_but_not_sufficient !== true ||
    proof.policy?.recipient_debit_must_equal_provider_credit !== true ||
    proof.policy?.unresolved_evidence_is_never_hidden !== true ||
    Number(proof.summary?.component_proofs) !== 4 ||
    Number(proof.summary?.components_with_exact_current_scalar) !== 4 ||
    Number(proof.summary?.components_with_matching_build_external_lifecycle) !== 4 ||
    Number(proof.summary?.offensive_components_runtime_enabled) !== 0 ||
    Number(proof.summary?.components_requiring_conservation_replay) !== 4 ||
    Number(proof.summary?.direct_summon_damage_transferred_to_support_rdps) !== 0) {
    throw new Error("Imagine formula proof policy or summary is unsafe");
  }
  const components = uniqueIndex(proof.components ?? [], "component_id", "Imagine formula component");
  const fatal = components.get("fatal-spiral-shared-all-element-bonus");
  const timeDecree = components.get("time-decree-external-cooldown-speed");
  const superStats = components.get("superconductor-surge-mechanical-power-main-stats");
  const superHealing = components.get("superconductor-surge-mechanical-power-healing-received");
  if (components.size !== 4 || !fatal || !timeDecree || !superStats || !superHealing ||
    Number(fatal.imagine_skill_id) !== 3957 ||
    stableStringify(fatal.effect_ids) !== stableStringify([2110125]) ||
    stableStringify(fatal.provider_marker_effect_ids) !== stableStringify([2110124]) ||
    stableStringify(fatal.excluded_owner_damage_ids) !== stableStringify([111007400108]) ||
    fatal.proof_state !== "current-build-tier-formula-and-packet-attribute-oracle-exact" ||
    fatal.exact_component_scalar_available !== true ||
    fatal.matching_build_external_lifecycle_observed !== true ||
    Number(fatal.fixed_point_denominator) !== 10000 || Number(fatal.base_attr_per) !== 500 ||
    fatal.equation !== "all_element_bonus_basis_points = 500 + tier_attr_per" ||
    stableStringify((fatal.tier_values ?? []).map((tier) => [
      Number(tier.tier), Number(tier.tier_attr_per), Number(tier.total_basis_points), Number(tier.percent),
    ])) !== stableStringify([
      [1, 100, 600, 6], [2, 200, 700, 7], [3, 300, 800, 8],
      [4, 400, 900, 9], [5, 500, 1000, 10],
    ]) ||
    Number(fatal.duration_millis) !== 10000 || Number(fatal.same_type_lockout_millis) !== 60000 ||
    Number(fatal.packet_attribute_oracle?.effect_id) !== 2110125 ||
    stableStringify(fatal.packet_attribute_oracle?.attribute_ids) !==
      stableStringify([13100, 13101, 13102]) ||
    Number(fatal.packet_attribute_oracle?.tier) !== 5 ||
    Number(fatal.packet_attribute_oracle?.applied_delta) !== 1000 ||
    Number(fatal.packet_attribute_oracle?.removed_delta) !== -1000 ||
    Number(fatal.packet_attribute_oracle?.correlated_status_events) !== 120 ||
    fatal.attribution_contract?.effect_provider_and_recipient_lifecycle_complete !== true ||
    fatal.attribution_contract?.equipped_provider_tier_snapshot_required !== true ||
    fatal.attribution_contract?.affected_hit_rows_selected !== false ||
    fatal.attribution_contract?.integer_damage_counterfactual_complete !== false ||
    fatal.attribution_contract?.current_build_conservation_replay_complete !== false ||
    fatal.attribution_contract?.runtime_rdps_enabled !== false) {
    throw new Error("Fatal Spiral Imagine formula component is unsafe or incomplete");
  }
  if (Number(timeDecree.imagine_skill_id) !== 3921 ||
    stableStringify(timeDecree.effect_ids) !== stableStringify([2110034]) ||
    timeDecree.proof_state !== "current-build-static-scalar-and-native-equation-exact" ||
    timeDecree.exact_component_scalar_available !== true ||
    timeDecree.exact_native_equation_available !== true ||
    timeDecree.matching_build_external_lifecycle_observed !== true ||
    stableStringify(timeDecree.tier_values) !==
      stableStringify({ 1: 10, 2: 20, 3: 30, 4: 40, 5: 50 }) ||
    Number(timeDecree.duration_millis) !== 20000 || Number(timeDecree.lockout_effect_id) !== 2110056 ||
    timeDecree.attribution_contract?.qualifying_skill_cooldown_category_map_complete !== false ||
    timeDecree.attribution_contract?.recipient_cast_schedule_replay_complete !== false ||
    timeDecree.attribution_contract?.cooldown_enabled_extra_casts_identified !== false ||
    timeDecree.attribution_contract?.recounted_child_events_conserved !== false ||
    timeDecree.attribution_contract?.current_build_conservation_replay_complete !== false ||
    timeDecree.attribution_contract?.runtime_rdps_enabled !== false) {
    throw new Error("Time Decree Imagine formula component is unsafe or incomplete");
  }
  const expectedTierPairs = {
    1: [780, 1040], 2: [960, 1280], 3: [1140, 1520],
    4: [1320, 1760], 5: [1500, 2000],
  };
  const expectedLoadoutTierPairs = {
    0: [750, 1000],
    ...expectedTierPairs,
  };
  const expectedStarIncrementPairs = {
    1: [150, 200], 2: [300, 400], 3: [450, 600],
    4: [600, 800], 5: [750, 1000],
  };
  const expectedClassTransforms = [
    [1, 11030, 11330, 11332, 1250, "1/8"],
    [2, 11020, 11340, 11342, 1000, "1/10"],
    [3, 11010, 11330, 11332, 1250, "1/8"],
    [4, 11010, 11330, 11332, 1250, "1/8"],
    [5, 11020, 11340, 11342, 1000, "1/10"],
    [9, 11010, 11330, 11332, 1250, "1/8"],
    [11, 11030, 11330, 11332, 1250, "1/8"],
    [12, 11010, 11330, 11332, 1250, "1/8"],
    [13, 11020, 11340, 11342, 1000, "1/10"],
  ];
  const observedClassTransforms = (superStats.current_class_primary_transforms ?? []).map(
    (route) => [
      Number(route.class_id),
      Number(route.primary_attribute_id),
      Number(route.attack_attribute_id),
      Number(route.attack_add_attribute_id),
      Number(route.coefficient_basis_points),
      String(route.exact_ratio),
    ],
  );
  const packetMarginal = superStats.packet_primary_attack_marginal_proof;
  if (Number(superStats.imagine_skill_id) !== 3971 ||
    stableStringify(superStats.effect_ids) !== stableStringify([2110140]) ||
    superStats.proof_state !==
      "current-build-tier-scalar-static-talent-routes-and-runtime-input-route-exact-packet-marginal-partial" ||
    superStats.exact_component_scalar_available !== true ||
    superStats.matching_build_external_lifecycle_observed !== true ||
    stableStringify(superStats.base_parameter_pair) !== stableStringify([750, 1000]) ||
    stableStringify(superStats.tier_parameter_pairs) !== stableStringify(expectedTierPairs) ||
    stableStringify(superStats.loadout_tier_parameter_pairs) !==
      stableStringify(expectedLoadoutTierPairs) ||
    stableStringify(superStats.star_increment_parameter_pairs) !==
      stableStringify(expectedStarIncrementPairs) ||
    Number(superStats.duration_millis) !== 15000 ||
    stableStringify(observedClassTransforms) !== stableStringify(expectedClassTransforms) ||
    (superStats.current_class_primary_transforms ?? []).some(
      (route) => Number(route.fixed_point_denominator) !== 10000 ||
        typeof route.formula !== "string" || !route.formula ||
        route.authority_scope !==
          "static talent opcode and class route only; not by itself the complete packet marginal of a support effect" ||
        route.rounding !== "integer truncation toward zero; primary stats are nonnegative, therefore floor",
    ) ||
    Number(packetMarginal?.class_id) !== 11 || Number(packetMarginal?.effect_id) !== 2110140 ||
    Number(packetMarginal?.primary_current_attribute_id) !== 11030 ||
    Number(packetMarginal?.primary_total_attribute_id) !== 11031 ||
    Number(packetMarginal?.primary_percent_attribute_id) !== 11034 ||
    Number(packetMarginal?.attack_add_attribute_id) !== 11332 ||
    Number(packetMarginal?.exact_lifecycle_windows) !== 8 ||
    Number(packetMarginal?.exact_transition_boundaries) !== 16 ||
    Number(packetMarginal?.exact_expression_matches) !== 15 ||
    Number(packetMarginal?.unresolved_same_packet_attack_percent_confounders) !== 1 ||
    Number(packetMarginal?.effective_stat_window_damage_actions) !== 12547 ||
    Number(packetMarginal?.static_1_over_8_complete_packet_marginal_matches) !== 0 ||
    packetMarginal?.complete_packet_marginal_proven_for_every_boundary !== false ||
    packetMarginal?.integer_damage_counterfactual_complete !== false ||
    packetMarginal?.provider_rdps_credit_allowed !== false ||
    superStats.historical_transition_guard?.current_build_effect_2110140_class_11_packet_ratio !==
      "58/100" ||
    superStats.attribution_contract?.event_time_recipient_class_selects_transform !== true ||
    superStats.attribution_contract?.recipient_pre_effect_attribute_snapshot_complete !== false ||
    superStats.attribution_contract?.recipient_effect_attribute_delta_replay_complete !== false ||
    superStats.attribution_contract?.exact_packet_transition_boundaries_complete_for_retained_lifecycles !== true ||
    Number(superStats.attribution_contract?.retained_lifecycle_count_with_exact_transition_boundaries) !== 8 ||
    Number(superStats.attribution_contract?.retained_effective_stat_window_damage_actions) !== 12547 ||
    superStats.attribution_contract?.packet_primary_attack_marginal_complete_for_all_boundaries !== false ||
    superStats.attribution_contract?.affected_hit_rows_selected !== false ||
    superStats.attribution_contract?.integer_damage_counterfactual_complete !== false ||
    superStats.attribution_contract?.current_build_conservation_replay_complete !== false ||
    superStats.attribution_contract?.runtime_rdps_enabled !== false ||
    Number(superHealing.imagine_skill_id) !== 3971 ||
    stableStringify(superHealing.effect_ids) !== stableStringify([2110140]) ||
    superHealing.proof_state !== "current-build-tier-parameter-and-external-lifecycle-exact" ||
    superHealing.exact_component_scalar_available !== true ||
    superHealing.matching_build_external_lifecycle_observed !== true ||
    stableStringify(superHealing.base_parameter_pair) !== stableStringify([750, 1000]) ||
    stableStringify(superHealing.tier_parameter_pairs) !== stableStringify(expectedTierPairs) ||
    stableStringify(superHealing.loadout_tier_parameter_pairs) !==
      stableStringify(expectedLoadoutTierPairs) ||
    stableStringify(superHealing.star_increment_parameter_pairs) !==
      stableStringify(expectedStarIncrementPairs) ||
    superHealing.attribution_contract?.lane !== "healing-only" ||
    superHealing.attribution_contract?.damage_credit_allowed !== false ||
    superHealing.attribution_contract?.effective_healing_counterfactual_complete !== false ||
    superHealing.attribution_contract?.overheal_replay_complete !== false ||
    superHealing.attribution_contract?.current_build_conservation_replay_complete !== false ||
    superHealing.attribution_contract?.runtime_rdps_enabled !== false) {
    throw new Error("Superconductor Surge Imagine formula components are unsafe or incomplete");
  }

  const proofReceipt = fileDescriptor(proofPath);
  const closedAuthority = {
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
  index.set(2110124, {
    proof: proofReceipt,
    imagine_skill_id: 3957,
    component_id: "fatal-spiral-caster-side-marker",
    disposition: "provider-side-routing-marker-never-count-or-transfer-as-damage",
    nontransfer_routing_marker_proven: true,
    observed_damage_reassigned_to_provider: 0,
    ...closedAuthority,
  });
  index.set(2110125, {
    proof: proofReceipt,
    imagine_skill_id: 3957,
    component_id: fatal.component_id,
    proof_state: fatal.proof_state,
    exact_component_scalar_available: true,
    fixed_point_denominator: 10000,
    equation: fatal.equation,
    tier_total_basis_points: fatal.tier_values.map((tier) => Number(tier.total_basis_points)),
    packet_oracle_tier: 5,
    packet_oracle_applied_delta: 1000,
    packet_oracle_removed_delta: -1000,
    provider_tier_snapshot_complete: false,
    affected_hit_rows_selected: false,
    integer_damage_counterfactual_complete: false,
    conservation_replay_complete: false,
    observed_damage_reassigned_to_provider: 0,
    ...closedAuthority,
  });
  index.set(2110034, {
    proof: proofReceipt,
    imagine_skill_id: 3921,
    component_id: timeDecree.component_id,
    proof_state: timeDecree.proof_state,
    exact_component_scalar_available: true,
    exact_native_equation_available: true,
    tier_values_percent: [10, 20, 30, 40, 50],
    qualifying_skill_cooldown_category_map_complete: false,
    recipient_cast_schedule_replay_complete: false,
    conservation_replay_complete: false,
    observed_damage_reassigned_to_provider: 0,
    ...closedAuthority,
  });
  index.set(2110140, {
    proof: proofReceipt,
    imagine_skill_id: 3971,
    component_ids: [superStats.component_id, superHealing.component_id],
    damage_component_proof_state: superStats.proof_state,
    healing_component_proof_state: superHealing.proof_state,
    exact_component_scalar_available: true,
    tier_parameter_pairs: structuredClone(expectedTierPairs),
    loadout_tier_parameter_pairs: structuredClone(expectedLoadoutTierPairs),
    class_primary_attack_transforms: structuredClone(
      superStats.current_class_primary_transforms,
    ),
    packet_primary_attack_marginal: structuredClone(packetMarginal),
    provider_tier_snapshot_complete: false,
    recipient_pre_effect_attribute_snapshot_complete: false,
    recipient_effect_attribute_delta_replay_complete: false,
    affected_hit_rows_selected: false,
    integer_damage_counterfactual_complete: false,
    healing_lane_damage_credit_allowed: false,
    conservation_replay_complete: false,
    observed_damage_reassigned_to_provider: 0,
    ...closedAuthority,
  });
  return index;
}

function validateFatalSpiralDamageStageFrontier(proof, proofPath, build) {
  if (!proof) return null;
  const closure = proof.proof_closure ?? {};
  const evidence = proof.reviewed_evidence ?? {};
  const gap = evidence.gap_bounded_source_lifecycles ?? {};
  const cohort = evidence.source_formula_cohort ?? {};
  const counterfactual = evidence.counterfactual_exhaustion ?? {};
  const consumerSearch = evidence.exact_build_consumer_search ?? {};
  const controlledAcquisition = evidence.controlled_pair_acquisition ?? {};
  const candidateEvaluation = evidence.automated_integer_candidate_evaluation ?? {};
  const retainedComparison = evidence.retained_capture_comparison_exhaustion ?? {};
  const recoveredPartial = evidence.recovered_partial_prefix_comparison_exhaustion ?? {};
  const controlledCaptureClient = evidence.controlled_capture_client_frontier ?? {};
  const trainingSceneAccess = evidence.training_scene_access_frontier ?? {};
  const candidateReadiness = evidence.sealed_candidate_readiness_frontier ?? {};
  const gapSafeOwnership = evidence.ownership_resolved_gap_safe_source_memberships ?? {};
  if (!proofPath || ![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17].includes(Number(proof.schema_version)) ||
    proof.generated_by !== "tools/bpsr-fatal-spiral-damage-stage-frontier.mjs" ||
    String(proof.game_build) !== String(build) || Number(proof.identity?.effect_id) !== 2110125 ||
    Number(proof.identity?.provider_marker_effect_id) !== 2110124 ||
    Number(proof.identity?.generic_element_attribute_id) !== 13100 ||
    stableStringify(proof.identity?.generic_element_attribute_family) !==
      stableStringify([13100, 13101, 13102, 13103, 13104, 13105]) ||
    Number(proof.identity?.fixed_point_denominator) !== 10000 ||
    proof.topology?.effect_edge !==
      "provider -> effect/status lifecycle -> recipient or enemy target" ||
    proof.topology?.damage_edge !==
      "recipient damage action -> recipient or enemy target" ||
    proof.topology?.source_side_join !== "effect endpoint equals damage actor" ||
    proof.topology?.target_side_join !== "effect endpoint equals damage target" ||
    proof.policy?.source_and_target_endpoints_are_independent !== true ||
    proof.policy?.endpoint_allegiance_is_inferred !== false ||
    proof.policy?.remote_player_cast_packets_are_required !== false ||
    proof.policy?.packet_absence_is_zero !== false ||
    proof.policy?.current_snapshots_may_rewrite_historical_runs !== false ||
    (Number(proof.schema_version) >= 13 &&
      (proof.policy?.exact_build_spatial_attribute_names_are_evidence_only !== true ||
        proof.policy?.spatial_attributes_are_not_excluded_from_counterfactual_matching !== true)) ||
    (Number(proof.schema_version) >= 14 &&
      (proof.policy?.component_index_and_count_are_formula_identity !== false ||
        proof.policy?.packet_source_mismatches_are_preserved_as_unresolved !== true)) ||
    (Number(proof.schema_version) >= 15 &&
      (proof.policy?.relative_spatial_relation_tolerances_are_diagnostic_only !== true ||
        proof.policy?.relative_spatial_relation_equality_is_not_full_spatial_equivalence_proof !== true)) ||
    (Number(proof.schema_version) >= 16 &&
      (proof.policy?.future_capture_capability_is_historical_observation_proof !== false ||
        proof.policy?.enclosing_aoi_entity_is_provider_without_separate_proof !== false)) ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    !isValidFileDescriptor(proof.inputs?.tier_window_proof) ||
    !isValidFileDescriptor(proof.inputs?.source_gap_window_audit) ||
    !isValidFileDescriptor(proof.inputs?.source_state_scaling_proof) ||
    !isValidFileDescriptor(proof.inputs?.source_formula_cohort) ||
    !isValidFileDescriptor(proof.inputs?.source_counterfactual_frontier) ||
    (Number(proof.schema_version) >= 2 &&
      !isValidFileDescriptor(proof.inputs?.all_element_damage_consumer_frontier)) ||
    (Number(proof.schema_version) >= 3 &&
      !isValidFileDescriptor(proof.inputs?.controlled_pair_acquisition_worklist)) ||
    (Number(proof.schema_version) >= 5 &&
      !isValidFileDescriptor(proof.inputs?.retained_capture_comparison_exhaustion)) ||
    (Number(proof.schema_version) >= 6 &&
      !isValidFileDescriptor(proof.inputs?.controlled_capture_client_frontier)) ||
    (Number(proof.schema_version) >= 7 &&
      !isValidFileDescriptor(proof.inputs?.training_scene_access_frontier)) ||
    (Number(proof.schema_version) >= 9 &&
      !isValidFileDescriptor(proof.inputs?.recovered_partial_prefix_frontier)) ||
    (Number(proof.schema_version) >= 11 &&
      !isValidFileDescriptor(proof.inputs?.candidate_readiness_frontier)) ||
    (Number(proof.schema_version) >= 17 &&
      !isValidFileDescriptor(proof.inputs?.gap_safe_lifecycle_action_summary)) ||
    Number(gap.complete_lifecycles) !== 29 ||
    Number(gap.lifecycles_cut_by_data_quality_boundary) !== 173 ||
    Number(gap.audited_damage_event_memberships) !== 27238 ||
    Number(gap.replay_matched_damage_event_memberships) !== 27238 ||
    Number(cohort.samples) !== 27001 ||
    Number(cohort.samples_with_generic_element_attribute) !== 6753 ||
    stableStringify(cohort.observed_generic_element_attribute_values) !==
      stableStringify([316, 1316]) ||
    Number(cohort.repeated_input_groups) !== 0 ||
    Number(counterfactual.samples) !== 27001 ||
    Number(counterfactual.exact_controlled_groups) !== 0 ||
    Number(counterfactual.relaxed_controlled_groups) !== 0 ||
    Number(counterfactual.near_controlled_target_pairs) !== 0 ||
    Number(counterfactual.near_controlled_source_pairs) !== 0 ||
    (Number(proof.schema_version) >= 2 &&
      (consumerSearch.exact_current_build_family_identity_proven !== true ||
        consumerSearch.exact_fixed_point_state_equations_proven !== true ||
        consumerSearch.generated_lua_name_search_exhausted !== true ||
        consumerSearch.selected_native_direct_call_search_exhausted !== true ||
        consumerSearch.server_damage_operator_present_in_reviewed_client_static_inventory !== false ||
        consumerSearch.executable_all_element_damage_consumer_proven !== false)) ||
    (Number(proof.schema_version) >= 3 &&
      (controlledAcquisition.exact_capture_contract_defined !== true ||
        controlledAcquisition.exact_integer_candidate_discriminator_defined !== true ||
        controlledAcquisition.current_controlled_pairs_available !== false ||
        Number(controlledAcquisition.primary_absent_attribute_value) !== 316 ||
        Number(controlledAcquisition.primary_present_attribute_value) !== 1316 ||
        Number(controlledAcquisition.primary_attribute_delta) !== 1000)) ||
    (Number(proof.schema_version) >= 4 &&
      (Number(candidateEvaluation.analyzer_schema_version) !== 17 ||
        candidateEvaluation.model_id !==
          "effect-2110125-source-all-element-current-final-multiplier-candidate" ||
        Number(candidateEvaluation.fixed_point_denominator) !== 10000 ||
        Number(candidateEvaluation.evaluated_variant_count) <= 0 ||
        Number(candidateEvaluation.deterministic_pairs) !== 0 ||
        Number(candidateEvaluation.compatible_floor_pairs) !== 0 ||
        Number(candidateEvaluation.compatible_nearest_half_up_pairs) !== 0 ||
        candidateEvaluation.candidate_selected !== false)) ||
    (Number(proof.schema_version) >= 5 &&
      (Number(retainedComparison.reviewed_current_build_rlogs) !== 26 ||
        Number(retainedComparison.effect_observed_rlogs) !== 6 ||
        Number(retainedComparison.observed_action_ids) !== 92 ||
        Number(retainedComparison.effect_observed_rlog_samples) !== 318602 ||
        Number(retainedComparison.all_reviewed_rlog_samples) !== 488546 ||
        Number(retainedComparison.additional_absent_search_samples) !== 169944 ||
        Number(retainedComparison.exact_effect_present_groups) !== 68110 ||
        Number(retainedComparison.broad_diagnostic_absent_pairs) !== 12176 ||
        Number(retainedComparison.controlled_pairs) !== 0 ||
        Number(retainedComparison.evaluated_integer_candidate_pairs) !== 0 ||
        Number(retainedComparison.maximum_counterfactual_working_set_mib) > 512)) ||
    (Number(proof.schema_version) >= 6 &&
      (stableStringify(controlledCaptureClient.exact_server_command_templates) !==
          stableStringify([
            "addBuff",
            "delBuff",
            "addGMAttr",
            "clearGMAttr",
            "monsterForceUseSkill",
            "enterScene",
          ]) ||
        controlledCaptureClient.submission_route !== "zproxy.world_proxy.GMCommand" ||
        controlledCaptureClient.client_gate?.exact_build_returns_true !== true ||
        controlledCaptureClient.shipping_client_blocks_gm_submission !== true ||
        controlledCaptureClient.ordinary_production_account_server_authorization_proven !== false ||
        controlledCaptureClient.controlled_capture_currently_executable !== false)) ||
    (Number(proof.schema_version) >= 7 &&
      (stableStringify(trainingSceneAccess.training_scene_ids) !==
          stableStringify([10001, 10002]) ||
        Number(trainingSceneAccess.lua_chunks_scanned) !== 4821 ||
        Number(trainingSceneAccess.lua_parse_failures) !== 0 ||
        stableStringify(trainingSceneAccess.training_hall_identifier_files) !==
          stableStringify(["dmg_control_view.lua", "Global.lua"]) ||
        trainingSceneAccess.decoded_dungeon_entry_route_found !== false ||
        trainingSceneAccess.ordinary_ui_or_service_lua_entry_route_found !== false ||
        trainingSceneAccess.ordinary_production_access_proven !== false ||
        trainingSceneAccess.hidden_gm_entry_route_present !== true ||
        trainingSceneAccess.shipping_client_blocks_hidden_route !== true ||
        trainingSceneAccess.authorized_controlled_capture_route_currently_executable !== false)) ||
    (Number(proof.schema_version) >= 8 &&
      (consumerSearch.exact_native_immediate_family_search_exhausted !== true ||
        consumerSearch.combat_relevant_exact_family_immediate_consumer_found !== false ||
        consumerSearch.computed_indirect_table_driven_or_protected_consumer_excluded !== false)) ||
    (Number(proof.schema_version) >= 9 &&
      (Number(recoveredPartial.partial_input_count) !== 10 ||
        Number(recoveredPartial.recovered_nonempty_input_count) !== 9 ||
        Number(recoveredPartial.validated_prefix_events) !== 1039616 ||
        Number(recoveredPartial.derived_terminal_gap_events) !== 9 ||
        Number(recoveredPartial.recovered_canonical_events) !== 1039625 ||
        Number(recoveredPartial.complete_gap_bounded_lifecycles) !== 23 ||
        Number(recoveredPartial.safe_source_damage_memberships) !== 16376 ||
        Number(recoveredPartial.selected_damage_action_id_count) !== 89 ||
        Number(recoveredPartial.comparison_samples) !== 92161 ||
        Number(recoveredPartial.controlled_pairs) !== 0 ||
        Number(recoveredPartial.review_band_pairs) !== 57 ||
        Number(recoveredPartial.review_band_rejected_without_source_attribute_transition) !== 57 ||
        recoveredPartial.source_capture_integrity_seal_authority !== false)) ||
    (Number(proof.schema_version) >= 10 &&
      (consumerSearch.exact_build_generic_instantiation_indexed !== true ||
        consumerSearch.bounded_direct_getter_call_search_exhausted !== true ||
        consumerSearch.combat_relevant_literal_attribute_getter_consumer_found !== false ||
        consumerSearch.exact_method_pointer_slot_inventory_complete !== true ||
        consumerSearch.exact_rip_relative_slot_reference_search_exhausted !== true ||
        consumerSearch.indexed_metadata_dispatch_or_protected_consumer_excluded !== false)) ||
    (Number(proof.schema_version) >= 11 &&
      (candidateReadiness.recursive_sealed_rlog_discovery_bounded !== true ||
        candidateReadiness.exact_build_and_canonical_seal_required !== true ||
        candidateReadiness.known_seal_deduplication_complete !== true ||
        candidateReadiness.unseen_seal_positive_control_triggers_refresh !== true ||
        candidateReadiness.source_side_effect_endpoint_join_complete !== true ||
        Number(candidateReadiness.current_new_candidate_rlogs) !== 0 ||
        Number(candidateReadiness.source_transition_same_context_pairs) !== 229 ||
        Number(candidateReadiness.source_transition_minimum_residual_observed_state_dimensions) !==
          (Number(proof.schema_version) >= 12 ? 13 : 14) ||
        Number(candidateReadiness.current_strict_controlled_pairs) !== 0)) ||
    (Number(proof.schema_version) >= 12 &&
      (candidateReadiness.configured_endpoint_attribute_family_diagnostic_complete !== true ||
        Number(candidateReadiness.source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic) !== 14 ||
        Number(candidateReadiness.source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions) !== 0)) ||
    (Number(proof.schema_version) >= 13 &&
      (candidateReadiness.configured_endpoint_transition_residual_ranking_complete !== true ||
        candidateReadiness.exact_build_spatial_attribute_identity_proof_complete !== true ||
        candidateReadiness.retained_spatial_raw_value_replay_complete !== true ||
        candidateReadiness.spatial_attributes_safe_to_exclude_from_counterfactual_matching !== false ||
        Number(candidateReadiness.configured_endpoint_transition_pairs) !== 69 ||
        Number(candidateReadiness.configured_endpoint_transition_minimum_residual_dimensions) !== 13 ||
        stableStringify(candidateReadiness.exact_build_spatial_attribute_identities) !==
          stableStringify({ 52: "AttrPos", 53: "AttrTargetPos" }) ||
        stableStringify(candidateReadiness.spatial_attribute_observations) !==
          stableStringify({ 52: 132297, 53: 142877 }) ||
        stableStringify(candidateReadiness.spatial_attribute_position_decodes) !==
          stableStringify({ 52: 131558, 53: 142130 }))) ||
    (Number(proof.schema_version) >= 14 &&
      (candidateReadiness.exact_build_action_selector_roster_complete !== true ||
        Number(candidateReadiness.packet_source_compatible_static_formula_candidates) !== 6 ||
        candidateReadiness.packet_source_mismatch_preserved_unresolved !== true ||
        Number(candidateReadiness.exact_action_selectors) !== 7 ||
        Number(candidateReadiness.packet_source_route_matched_transition_pairs) !== 68 ||
        Number(candidateReadiness.packet_source_route_rejected_transition_pairs) !== 1)) ||
    (Number(proof.schema_version) >= 15 &&
      (candidateReadiness.relative_spatial_relation_audit_complete !== true ||
        candidateReadiness.direct_source_to_target_geometry_equal_for_all_complete_pairs !== false ||
        Number(candidateReadiness.direct_spatial_relation_complete_transition_pairs) !== 66 ||
        Number(candidateReadiness.direct_spatial_relation_exact_transition_pairs) !== 2 ||
        Number(candidateReadiness.direct_spatial_relation_nonexact_transition_pairs) !== 64 ||
        Number(candidateReadiness.direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12)) ||
    (Number(proof.schema_version) >= 16 &&
      (Number(candidateReadiness.fake_bullet_exact_wire_fields) !== 7 ||
        candidateReadiness.fake_bullet_future_capture_join_keys_retained !== true ||
        Number(candidateReadiness.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
        candidateReadiness.fake_bullet_source4_damage_route_resolved !== false ||
        candidateReadiness.fake_bullet_provider_ownership_proven !== false)) ||
    (Number(proof.schema_version) >= 17 &&
      (Number(gapSafeOwnership.gap_safe_windows) !== 29 ||
        Number(gapSafeOwnership.damage_memberships) !== 27238 ||
        Number(gapSafeOwnership.unique_damage_events) !== 27238 ||
        Number(gapSafeOwnership.third_party_provider_memberships) !== 25679 ||
        Number(gapSafeOwnership.provider_self_memberships) !== 1559 ||
        Number(gapSafeOwnership.ownership_unresolved_memberships) !== 0 ||
        String(gapSafeOwnership.reported_damage_membership_sum) !== "3501998794" ||
        stableStringify(gapSafeOwnership.source_protocol_pack_digests) !==
          stableStringify([
            "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
          ]))) ||
    closure.exact_event_time_provider_tier_join_complete !== true ||
    closure.exact_source_side_effect_recipient_to_damage_actor_join_complete !== true ||
    closure.exact_gap_bounded_lifecycle_replay_complete !== true ||
    closure.audited_damage_membership_selection_conserved !== true ||
    closure.controlled_pair_search_exhausted_for_retained_cohort !== true ||
    (Number(proof.schema_version) >= 2 &&
      closure.exact_build_client_consumer_search_exhausted !== true) ||
    (Number(proof.schema_version) >= 2 &&
      closure.exact_build_server_operator_absence_recorded !== true) ||
    (Number(proof.schema_version) >= 3 &&
      closure.exact_controlled_pair_acquisition_contract_defined !== true) ||
    (Number(proof.schema_version) >= 3 &&
      closure.exact_integer_candidate_discriminator_defined !== true) ||
    (Number(proof.schema_version) >= 3 &&
      closure.current_controlled_pairs_available !== false) ||
    (Number(proof.schema_version) >= 4 &&
      closure.automatic_integer_candidate_evaluator_integrated !== true) ||
    (Number(proof.schema_version) >= 5 &&
      closure.retained_current_build_present_and_absent_capture_frontier_exhausted !== true) ||
    (Number(proof.schema_version) >= 6 &&
      (closure.exact_build_hidden_controlled_capture_surface_identified !== true ||
        closure.shipping_client_blocks_controlled_capture_submission !== true ||
        closure.ordinary_production_account_controlled_capture_authorized !== false)) ||
    (Number(proof.schema_version) >= 7 &&
      (closure.exact_build_training_scene_access_frontier_reviewed !== true ||
        closure.ordinary_training_scene_entry_route_proven !== false ||
        closure.training_scene_controlled_capture_currently_executable !== false)) ||
    (Number(proof.schema_version) >= 8 &&
      (closure.exact_native_immediate_family_search_exhausted !== true ||
        closure.combat_relevant_exact_family_immediate_consumer_found !== false ||
        closure.computed_indirect_table_driven_or_protected_consumer_excluded !== false)) ||
    (Number(proof.schema_version) >= 9 &&
      (closure.retained_recovered_partial_prefix_frontier_exhausted !== true ||
        closure.recovered_partial_prefix_source_capture_integrity_seal_authority !== false)) ||
    (Number(proof.schema_version) >= 10 &&
      (closure.exact_build_generic_instantiation_indexed !== true ||
        closure.bounded_direct_getter_call_search_exhausted !== true ||
        closure.combat_relevant_literal_attribute_getter_consumer_found !== false ||
        closure.exact_method_pointer_slot_inventory_complete !== true ||
        closure.exact_rip_relative_slot_reference_search_exhausted !== true ||
        closure.indexed_metadata_dispatch_or_protected_consumer_excluded !== false)) ||
    (Number(proof.schema_version) >= 11 &&
      (closure.recursive_sealed_rlog_candidate_discovery_bounded !== true ||
        closure.exact_build_and_canonical_seal_candidate_gate_complete !== true ||
        closure.known_candidate_seal_deduplication_complete !== true ||
        closure.unseen_seal_positive_control_triggers_refresh !== true ||
        closure.source_transition_candidate_search_complete !== true ||
        Number(closure.current_new_sealed_candidate_rlogs) !== 0 ||
        Number(closure.current_source_transition_same_context_pairs) !== 229 ||
        Number(closure.current_source_transition_minimum_residual_observed_state_dimensions) !==
          (Number(proof.schema_version) >= 12 ? 13 : 14) ||
        Number(closure.current_source_transition_strict_controlled_pairs) !== 0)) ||
    (Number(proof.schema_version) >= 12 &&
      (closure.configured_endpoint_attribute_family_diagnostic_complete !== true ||
        Number(closure.current_source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic) !== 14 ||
        Number(closure.current_source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions) !== 0)) ||
    (Number(proof.schema_version) >= 13 &&
      (closure.configured_endpoint_transition_residual_ranking_complete !== true ||
        closure.exact_build_spatial_attribute_identity_proof_complete !== true ||
        closure.retained_spatial_raw_value_replay_complete !== true ||
        closure.spatial_attributes_safe_to_exclude_from_counterfactual_matching !== false ||
        Number(closure.current_configured_endpoint_transition_pairs) !== 69 ||
        Number(closure.current_configured_endpoint_transition_minimum_residual_dimensions) !== 13)) ||
    (Number(proof.schema_version) >= 14 &&
      (closure.exact_build_action_selector_roster_complete !== true ||
        Number(closure.packet_source_compatible_static_formula_candidates) !== 6 ||
        closure.packet_source_mismatch_preserved_unresolved !== true ||
        Number(closure.current_exact_action_selectors) !== 7 ||
        Number(closure.current_packet_source_route_matched_transition_pairs) !== 68 ||
        Number(closure.current_packet_source_route_rejected_transition_pairs) !== 1)) ||
    (Number(proof.schema_version) >= 15 &&
      (closure.relative_spatial_relation_audit_complete !== true ||
        closure.direct_source_to_target_geometry_equal_for_all_complete_pairs !== false ||
        Number(closure.current_direct_spatial_relation_complete_transition_pairs) !== 66 ||
        Number(closure.current_direct_spatial_relation_exact_transition_pairs) !== 2 ||
        Number(closure.current_direct_spatial_relation_nonexact_transition_pairs) !== 64 ||
        Number(closure.current_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12)) ||
    (Number(proof.schema_version) >= 16 &&
      (closure.fake_bullet_exact_wire_contract_complete !== true ||
        closure.fake_bullet_future_capture_timeline_preservation_complete !== true ||
        Number(closure.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
        closure.fake_bullet_historical_canonical_logs_backfilled !== false ||
        closure.fake_bullet_source4_damage_route_resolved !== false ||
        closure.fake_bullet_provider_ownership_proven !== false)) ||
    (Number(proof.schema_version) >= 17 &&
      (closure.exact_gap_safe_lifecycle_action_membership_join_complete !== true ||
        closure.provider_ownership_proven_for_all_gap_safe_memberships !== true ||
        Number(closure.gap_safe_damage_memberships) !== 27238 ||
        Number(closure.gap_safe_third_party_provider_memberships) !== 25679 ||
        Number(closure.gap_safe_provider_self_memberships) !== 1559 ||
        Number(closure.gap_safe_ownership_unresolved_memberships) !== 0)) ||
    closure.combat_damage_stage_consumer_proven !== false ||
    closure.exact_multiplier_application_proven !== false ||
    closure.exact_operation_order_proven !== false ||
    closure.exact_integer_rounding_proven !== false ||
    closure.multi_provider_stacking_and_split_proven !== false ||
    closure.integer_damage_counterfactual_projection_complete !== false ||
    closure.recipient_debit_provider_credit_conservation_complete !== false ||
    closure.formula_authority !== false || closure.runtime_authority !== false ||
    closure.ui_display_authority !== false || closure.provider_rdps_credit_allowed !== false ||
    Number(closure.observed_damage_reassigned_to_provider) !== 0) {
    throw new Error("Fatal Spiral damage-stage frontier is unsafe or incomplete");
  }
  return {
    proof: fileDescriptor(proofPath),
    tier_window_proof: structuredClone(proof.inputs.tier_window_proof),
    source_gap_window_audit: structuredClone(proof.inputs.source_gap_window_audit),
    source_state_scaling_proof: structuredClone(proof.inputs.source_state_scaling_proof),
    source_formula_cohort: structuredClone(proof.inputs.source_formula_cohort),
    source_counterfactual_frontier:
      structuredClone(proof.inputs.source_counterfactual_frontier),
    ...(Number(proof.schema_version) >= 2 ? {
      all_element_damage_consumer_frontier:
        structuredClone(proof.inputs.all_element_damage_consumer_frontier),
      exact_build_client_consumer_search_exhausted: true,
      exact_build_server_operator_absence_recorded: true,
    } : {}),
    ...(Number(proof.schema_version) >= 3 ? {
      controlled_pair_acquisition_worklist:
        structuredClone(proof.inputs.controlled_pair_acquisition_worklist),
      exact_controlled_pair_acquisition_contract_defined: true,
      exact_integer_candidate_discriminator_defined: true,
      current_controlled_pairs_available: false,
    } : {}),
    ...(Number(proof.schema_version) >= 4 ? {
      automatic_integer_candidate_evaluator_integrated: true,
      automatic_integer_candidate_model_id: candidateEvaluation.model_id,
      automatic_integer_candidate_analyzer_schema_version:
        Number(candidateEvaluation.analyzer_schema_version),
      automatic_integer_candidate_evaluated_variants:
        Number(candidateEvaluation.evaluated_variant_count),
    } : {}),
    ...(Number(proof.schema_version) >= 5 ? {
      retained_capture_comparison_exhaustion:
        structuredClone(proof.inputs.retained_capture_comparison_exhaustion),
      retained_current_build_present_and_absent_capture_frontier_exhausted: true,
      reviewed_current_build_rlogs: Number(retainedComparison.reviewed_current_build_rlogs),
      observed_damage_action_ids: Number(retainedComparison.observed_action_ids),
      all_reviewed_comparison_samples:
        Number(retainedComparison.all_reviewed_rlog_samples),
      broad_diagnostic_absent_pairs:
        Number(retainedComparison.broad_diagnostic_absent_pairs),
    } : {}),
    ...(Number(proof.schema_version) >= 6 ? {
      controlled_capture_client_frontier:
        structuredClone(proof.inputs.controlled_capture_client_frontier),
      exact_build_hidden_controlled_capture_surface_identified: true,
      shipping_client_blocks_controlled_capture_submission: true,
      ordinary_production_account_controlled_capture_authorized: false,
      controlled_capture_currently_executable: false,
    } : {}),
    ...(Number(proof.schema_version) >= 7 ? {
      training_scene_access_frontier:
        structuredClone(proof.inputs.training_scene_access_frontier),
      exact_build_training_scene_access_frontier_reviewed: true,
      ordinary_training_scene_entry_route_proven: false,
      training_scene_controlled_capture_currently_executable: false,
    } : {}),
    ...(Number(proof.schema_version) >= 8 ? {
      exact_native_immediate_family_search_exhausted: true,
      combat_relevant_exact_family_immediate_consumer_found: false,
      computed_indirect_table_driven_or_protected_consumer_excluded: false,
    } : {}),
    ...(Number(proof.schema_version) >= 9 ? {
      recovered_partial_prefix_frontier:
        structuredClone(proof.inputs.recovered_partial_prefix_frontier),
      retained_recovered_partial_prefix_frontier_exhausted: true,
      recovered_partial_prefix_source_capture_integrity_seal_authority: false,
      recovered_partial_prefix_validated_events:
        Number(recoveredPartial.validated_prefix_events),
      recovered_partial_prefix_comparison_samples:
        Number(recoveredPartial.comparison_samples),
      recovered_partial_prefix_controlled_pairs: Number(recoveredPartial.controlled_pairs),
    } : {}),
    ...(Number(proof.schema_version) >= 10 ? {
      exact_build_generic_instantiation_indexed: true,
      bounded_direct_getter_call_search_exhausted: true,
      combat_relevant_literal_attribute_getter_consumer_found: false,
      exact_method_pointer_slot_inventory_complete: true,
      exact_rip_relative_slot_reference_search_exhausted: true,
      indexed_metadata_dispatch_or_protected_consumer_excluded: false,
    } : {}),
    ...(Number(proof.schema_version) >= 11 ? {
      candidate_readiness_frontier:
        structuredClone(proof.inputs.candidate_readiness_frontier),
      recursive_sealed_rlog_candidate_discovery_bounded: true,
      exact_build_and_canonical_seal_candidate_gate_complete: true,
      known_candidate_seal_deduplication_complete: true,
      unseen_seal_positive_control_triggers_refresh: true,
      source_transition_candidate_search_complete: true,
      current_new_sealed_candidate_rlogs: 0,
      current_source_transition_same_context_pairs: 229,
      current_source_transition_minimum_residual_observed_state_dimensions:
        Number(proof.schema_version) >= 12 ? 13 : 14,
      current_source_transition_strict_controlled_pairs: 0,
    } : {}),
    ...(Number(proof.schema_version) >= 12 ? {
      configured_endpoint_attribute_family_diagnostic_complete: true,
      current_source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic: 14,
      current_source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions: 0,
    } : {}),
    ...(Number(proof.schema_version) >= 13 ? {
      configured_endpoint_transition_residual_ranking_complete: true,
      exact_build_spatial_attribute_identity_proof_complete: true,
      retained_spatial_raw_value_replay_complete: true,
      spatial_attributes_safe_to_exclude_from_counterfactual_matching: false,
      current_configured_endpoint_transition_pairs: 69,
      current_configured_endpoint_transition_minimum_residual_dimensions: 13,
      exact_build_spatial_attribute_identities: { 52: "AttrPos", 53: "AttrTargetPos" },
      spatial_attribute_observations: { 52: 132297, 53: 142877 },
      spatial_attribute_position_decodes: { 52: 131558, 53: 142130 },
    } : {}),
    ...(Number(proof.schema_version) >= 14 ? {
      exact_build_action_selector_roster_complete: true,
      packet_source_compatible_static_formula_candidates: 6,
      packet_source_mismatch_preserved_unresolved: true,
      current_exact_action_selectors: 7,
      current_packet_source_route_matched_transition_pairs: 68,
      current_packet_source_route_rejected_transition_pairs: 1,
    } : {}),
    ...(Number(proof.schema_version) >= 15 ? {
      relative_spatial_relation_audit_complete: true,
      direct_source_to_target_geometry_equal_for_all_complete_pairs: false,
      current_direct_spatial_relation_complete_transition_pairs: 66,
      current_direct_spatial_relation_exact_transition_pairs: 2,
      current_direct_spatial_relation_nonexact_transition_pairs: 64,
      current_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs: 12,
    } : {}),
    ...(Number(proof.schema_version) >= 16 ? {
      fake_bullet_exact_wire_contract_complete: true,
      fake_bullet_future_capture_timeline_preservation_complete: true,
      fake_bullet_current_build_observed_lifecycle_records: 0,
      fake_bullet_historical_canonical_logs_backfilled: false,
      fake_bullet_source4_damage_route_resolved: false,
      fake_bullet_provider_ownership_proven: false,
    } : {}),
    ...(Number(proof.schema_version) >= 17 ? {
      gap_safe_lifecycle_action_summary:
        structuredClone(proof.inputs.gap_safe_lifecycle_action_summary),
      exact_gap_safe_lifecycle_action_membership_join_complete: true,
      provider_ownership_proven_for_all_gap_safe_memberships: true,
      gap_safe_damage_memberships: Number(gapSafeOwnership.damage_memberships),
      gap_safe_third_party_provider_memberships:
        Number(gapSafeOwnership.third_party_provider_memberships),
      gap_safe_provider_self_memberships:
        Number(gapSafeOwnership.provider_self_memberships),
      gap_safe_ownership_unresolved_memberships:
        Number(gapSafeOwnership.ownership_unresolved_memberships),
      gap_safe_reported_damage_membership_sum:
        String(gapSafeOwnership.reported_damage_membership_sum),
      gap_safe_source_protocol_pack_digests:
        structuredClone(gapSafeOwnership.source_protocol_pack_digests),
    } : {}),
    exact_source_side_effect_recipient_to_damage_actor_join_complete: true,
    exact_gap_bounded_lifecycles: 29,
    audited_damage_event_memberships: 27238,
    retained_formula_samples: 27001,
    observed_generic_element_attribute_values: [316, 1316],
    controlled_counterfactual_pairs: 0,
    damage_stage_consumer_proven: false,
    operation_order_proven: false,
    integer_rounding_proven: false,
    conservation_replay_complete: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
    observed_damage_reassigned_to_provider: 0,
  };
}

function validateImagineStatusAttributeTierProof(
  proof,
  proofPath,
  imagineFormulaProofPath,
  build,
) {
  if (!proof) return null;
  if (!proofPath || proof.schema_version !== 1 ||
    proof.generated_by !== "tools/rdps-imagine-status-attribute-tier-proof.mjs" ||
    String(proof.game_build) !== String(build) || Number(proof.effect_id) !== 2110140 ||
    Number(proof.imagine_skill_id) !== 3971 ||
    proof.policy?.exact_numeric_ids_and_build_are_authoritative !== true ||
    proof.policy?.localized_names_are_evidence_only !== true ||
    proof.policy?.remote_cast_packet_required !== false ||
    proof.policy?.missing_remote_cast_is_synthesized !== false ||
    proof.policy?.tier_resolution_is_occurrence_scoped !== true ||
    proof.policy?.provider_tier_is_not_propagated_across_time_or_recipients !== true ||
    proof.policy?.unresolved_lifecycles_are_retained !== true ||
    proof.policy?.healing_received_is_never_damage_rdps !== true ||
    proof.policy?.formula_authority !== false || proof.policy?.runtime_authority !== false ||
    proof.policy?.ui_display_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    !isValidFileDescriptor(proof.inputs?.current_skill_fight_level_table) ||
    !isValidFileDescriptor(proof.inputs?.current_skill_aoyi_star_table) ||
    !isValidFileDescriptor(proof.inputs?.status_attribute_proof) ||
    !isValidFileDescriptor(proof.inputs?.provider_ownership_proof) ||
    stableStringify(proof.exact_current_build_loadout_tier_parameter_pairs) !==
      stableStringify({
        0: [750, 1000], 1: [780, 1040], 2: [960, 1280],
        3: [1140, 1520], 4: [1320, 1760], 5: [1500, 2000],
      }) ||
    Number(proof.attribute_contract?.main_stat_raw_percent_attribute_id) !== 11034 ||
    Number(proof.attribute_contract?.healing_received_add_attribute_id) !== 11802 ||
    Number(proof.summary?.selected_status_events) !== 272 ||
    Number(proof.summary?.applied_status_instances) !== 136 ||
    Number(proof.summary?.removed_status_instances) !== 136 ||
    Number(proof.summary?.exact_paired_attribute_occurrences) !== 8 ||
    Number(proof.summary?.exact_base_tier_occurrences) !== 2 ||
    Number(proof.summary?.exact_tier_5_occurrences) !== 6 ||
    Number(proof.summary?.unresolved_applied_status_instances) !== 128 ||
    Number(proof.summary?.unmatched_clean_attribute_occurrences) !== 0 ||
    Number(proof.summary?.provider_groups) !== 2 ||
    Number(proof.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    !Array.isArray(proof.resolved_lifecycle_occurrences) ||
    proof.resolved_lifecycle_occurrences.length !== 8 ||
    proof.resolved_lifecycle_occurrences.some((entry) =>
      Number(entry.effect_id) !== 2110140 || ![0, 5].includes(Number(entry.loadout_tier)) ||
      Number(entry.exact_attribute_pair?.main_stat_raw_percent_attribute_id) !== 11034 ||
      Number(entry.exact_attribute_pair?.healing_received_add_attribute_id) !== 11802 ||
      entry.formula_authority !== false || entry.runtime_authority !== false ||
      entry.provider_rdps_credit_allowed !== false)) {
    throw new Error("Imagine status-attribute tier proof is unsafe or incomplete");
  }
  return {
    proof: fileDescriptor(proofPath),
    status_attribute_proof: structuredClone(proof.inputs.status_attribute_proof),
    provider_ownership_proof: structuredClone(proof.inputs.provider_ownership_proof),
    resolution_scope: "exact-provider-status-instance-recipient-lifecycle-occurrence-only",
    exact_paired_attribute_occurrences: 8,
    exact_base_tier_occurrences: 2,
    exact_tier_5_occurrences: 6,
    unresolved_applied_status_instances: 128,
    tier_zero_parameter_pair: [750, 1000],
    tier_five_parameter_pair: [1500, 2000],
    provider_tier_snapshot_complete: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
    observed_damage_reassigned_to_provider: 0,
  };
}

function validateImagineTierWindowCounterfactualInputs(
  proof,
  proofPath,
  tierProofPath,
  imagineFormulaProofPath,
  build,
) {
  if (!proof) return null;
  const windows = proof.lifecycle_windows ?? [];
  const actions = windows.flatMap((window) => window.damage_actions ?? []);
  if (!proofPath || !tierProofPath ||
    proof.schema_version !== 1 ||
    proof.generated_by !== "tools/rdps-imagine-tier-window-counterfactual-inputs.mjs" ||
    String(proof.game_build) !== String(build) || Number(proof.effect_id) !== 2110140 ||
    Number(proof.imagine_skill_id) !== 3971 ||
    proof.topology?.effect_edge !==
      "provider -> effect/status lifecycle -> recipient or enemy target" ||
    proof.topology?.damage_edge !==
      "recipient damage action -> recipient or enemy target" ||
    proof.topology?.source_side_join !==
      "effect affected entity equals damage actor" ||
    proof.topology?.allegiance_assumptions !== false ||
    proof.policy?.exact_numeric_ids_and_build_are_authoritative !== true ||
    proof.policy?.localized_names_are_evidence_only !== true ||
    proof.policy?.remote_player_cast_packets_required !== false ||
    proof.policy?.remote_player_cast_packets_synthesized !== false ||
    proof.policy?.status_tier_resolution_is_occurrence_scoped !== true ||
    proof.policy?.tier_propagation_across_lifecycles_or_recipients !== false ||
    proof.policy?.damage_actions_are_retained_counterfactual_inputs_only !== true ||
    proof.policy?.damage_endpoint_is_assumed_enemy !== false ||
    proof.policy?.damage_endpoint_is_assumed_friendly !== false ||
    proof.policy?.integer_damage_stage_order_and_rounding_proven !== false ||
    proof.policy?.ordinary_damage_totals_changed !== false ||
    Number(proof.policy?.observed_damage_reassigned_to_provider) !== 0 ||
    proof.policy?.formula_authority !== false || proof.policy?.runtime_authority !== false ||
    proof.policy?.ui_display_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    stableStringify(proof.inputs?.tier_proof) !== stableStringify(fileDescriptor(tierProofPath)) ||
    !isValidFileDescriptor(proof.inputs?.recipient_snapshots) ||
    !isValidFileDescriptor(proof.inputs?.support_timeline) ||
    Number(proof.inputs?.support_timeline?.schema_version) !== 9 ||
    Number(proof.inputs?.support_timeline?.lines) !== 4678442 ||
    Number(proof.summary?.tier_resolved_lifecycles) !== 8 ||
    Number(proof.summary?.exact_apply_remove_windows) !== 8 ||
    Number(proof.summary?.complete_window_inputs) !== 8 ||
    Number(proof.summary?.unresolved_window_inputs) !== 0 ||
    Number(proof.summary?.retained_recipient_damage_actions) !== 12557 ||
    String(proof.summary?.retained_hp_loss) !== "1923279061" ||
    String(proof.summary?.retained_reported_damage) !== "1947659979" ||
    Number(proof.summary?.single_effect_provider_damage_actions) !== 12557 ||
    Number(proof.summary?.concurrent_effect_provider_damage_actions) !== 0 ||
    Number(proof.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    windows.length !== 8 || actions.length !== 12557 ||
    windows.some((window) =>
      Number(window.effect_id) !== 2110140 ||
      ![0, 5].includes(Number(window.loadout_tier)) ||
      window.lifecycle_state !== "exact-apply-remove" ||
      window.window_input_state !== "complete" ||
      window.recipient_formula_input_snapshot?.state !== "complete" ||
      Number(window.recipient_formula_input_snapshot?.class_id) !== 11 ||
      window.counterfactual_damage_delta !== null ||
      String(window.provider_rdps_credit) !== "0" ||
      window.provider_rdps_credit_allowed !== false ||
      (window.damage_actions ?? []).some((action) =>
        String(action.damage_actor_entity_uuid) !== String(window.affected_entity_uuid) ||
        action.damage_endpoint_allegiance !== "unresolved" ||
        String(action.provider_rdps_credit) !== "0")) ||
    actions.some((action) => !Array.isArray(action.concurrent_effect_2110140_instances) ||
      action.concurrent_effect_2110140_instances.length !== 1)) {
    throw new Error("Imagine tier-window counterfactual inputs are unsafe or incomplete");
  }
  return {
    proof: fileDescriptor(proofPath),
    recipient_snapshots: structuredClone(proof.inputs.recipient_snapshots),
    support_timeline: structuredClone(proof.inputs.support_timeline),
    resolution_scope: "eight-exact-provider-status-instance-recipient-windows-only",
    exact_apply_remove_windows: 8,
    complete_window_inputs: 8,
    retained_recipient_damage_actions: 12557,
    retained_hp_loss: "1923279061",
    retained_reported_damage: "1947659979",
    single_effect_provider_damage_actions: 12557,
    concurrent_effect_provider_damage_actions: 0,
    global_provider_tier_snapshot_complete: false,
    global_recipient_pre_effect_attribute_snapshot_complete: false,
    retained_actions_have_counterfactual_damage_delta: false,
    integer_damage_stage_order_and_rounding_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
    observed_damage_reassigned_to_provider: 0,
  };
}

function buildIntegerTransformConstraintIndex(documents, documentPaths, build) {
  const index = new Map();
  for (let documentIndex = 0; documentIndex < documents.length; documentIndex += 1) {
    const document = documents[documentIndex];
    if (![1, 2, 3, 4, 5, 6, 7, 8, 9].includes(document.schema_version) ||
      document.generated_by !== "bpsr-rdps-integer-transform-constraints") {
      throw new Error("Unsupported status-effect integer transform constraint schema or generator");
    }
    requireBuild(document.game_build, build, "status-effect integer transform constraints");
    if (document.policy?.analysis_scope !== "final_observed_damage_integer_transform_constraints_only" ||
      document.policy?.candidate_models_are_compatibility_constraints_not_formula_proof !== true ||
      document.policy?.formula_stage_is_proven !== false ||
      document.policy?.operation_order_is_proven !== false ||
      document.policy?.stacking_is_proven !== false ||
      document.policy?.runtime_integer_rounding_is_proven !== false ||
      document.policy?.formula_authority !== false || document.policy?.runtime_authority !== false ||
      document.policy?.provider_rdps_credit_allowed !== false ||
      document.policy?.unresolved_evidence_is_preserved !== true ||
      document.interpretation?.exact_transform_proven !== false) {
      throw new Error("Status-effect integer transform constraint authority policy is unsafe");
    }
    const effectId = Number(document.effect_id);
    const locus = String(document.locus ?? "");
    const key = `${locus}:${effectId}`;
    if (!Number.isSafeInteger(effectId) || effectId <= 0 || !["source", "target"].includes(locus) ||
      index.has(key) || Number(document.observation_summary?.exact_divergent_controlled_examples ?? 0) <= 0) {
      throw new Error("Status-effect integer transform constraints have an invalid or duplicate effect locus");
    }
    const candidates = document.compatible_model_intersection
      ?.post_output_multiplicative_delta_basis_points;
    if (!["floor", "round_half_up", "ceil"].every((mode) =>
      Array.isArray(candidates?.[mode]) && candidates[mode].every(Number.isSafeInteger)
    )) {
      throw new Error(`Status-effect integer transform constraints ${key} have invalid candidates`);
    }
    const staticFormulaInputCandidates = document.schema_version >= 2
      ? validateStaticFormulaInputCandidates(document, effectId, key)
      : null;
    const providerFormulaContextSummary = document.schema_version >= 4
      ? validateProviderFormulaContextSummary(document, key)
      : null;
    const providerFormulaInputCoverage = document.schema_version >= 5
      ? validateProviderFormulaInputCoverage(document, effectId, key)
      : null;
    const spHealOperatorEvidence = document.schema_version >= 6
      ? validateSpHealOperatorEvidence(document, effectId, key)
      : null;
    const eventLocalCounterfactualConservation = document.schema_version >= 8
      ? validateEventLocalCounterfactualConservation(document, effectId, key)
      : null;
    const nearControlledExhaustion = document.schema_version >= 9
      ? validateNearControlledExhaustion(document, effectId, key)
      : null;
    index.set(key, {
      effect_id: String(effectId),
      locus,
      proof: fileDescriptor(documentPaths[documentIndex]),
      exact_divergent_controlled_examples:
        Number(document.observation_summary.exact_divergent_controlled_examples),
      compatible_model_intersection: structuredClone(document.compatible_model_intersection),
      compatible_models_are_not_unique:
        document.interpretation?.compatible_models_are_not_unique === true,
      exact_transform_proven: false,
      exact_static_status_transform_binding_proven: false,
      static_formula_input_candidates: staticFormulaInputCandidates,
      provider_formula_context_summary: providerFormulaContextSummary,
      provider_formula_input_coverage: providerFormulaInputCoverage,
      spheal_operator_evidence: spHealOperatorEvidence,
      event_local_counterfactual_conservation: eventLocalCounterfactualConservation,
      near_controlled_exhaustion: nearControlledExhaustion,
      observed_rlogs: uniqueSorted((document.observations ?? []).map((entry) =>
        String(entry.rlog ?? "")
      ).filter(Boolean)),
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    });
  }
  return index;
}

function validateEventLocalCounterfactualConservation(document, effectId, key) {
  const proof = document.event_local_counterfactual_conservation;
  const summary = proof?.summary;
  const interpretation = proof?.interpretation;
  const events = proof?.events;
  if (document.policy
      ?.event_local_counterfactual_arithmetic_never_grants_causal_formula_or_runtime_authority !== true ||
    proof?.scope !== "single_exact_controlled_observed_damage_event_pairs" ||
    !Array.isArray(events) || events.length === 0 ||
    Number(summary?.event_pairs) !== events.length ||
    summary?.arithmetic_conservation_holds_for_every_pair !== true ||
    summary?.exact_recorded_inputs_controlled_for_every_pair !== true ||
    summary?.provider_player_identity_proven_for_every_pair !== true ||
    interpretation?.event_local_counterfactual_arithmetic_conservation_proven !== true ||
    interpretation?.observed_controlled_delta_is_not_a_general_formula !== true ||
    interpretation?.causal_provider_contribution_proven !== false ||
    interpretation?.exact_transform_proven !== false ||
    interpretation?.formula_stage_and_operation_order_proven !== false ||
    interpretation?.runtime_integer_rounding_proven !== false ||
    interpretation?.canonical_party_replay_conservation_proven !== false ||
    interpretation?.formula_authority !== false || interpretation?.runtime_authority !== false ||
    interpretation?.ui_authority !== false ||
    interpretation?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(interpretation?.next_required_evidence) ||
    interpretation.next_required_evidence.length === 0) {
    throw new Error(`Status-effect integer transform constraints ${key} have unsafe event-local conservation evidence`);
  }
  for (const event of events) {
    const absent = exactIntegerText(event.absent_damage, `${key} absent damage`);
    const delta = exactIntegerText(event.observed_controlled_delta, `${key} controlled delta`);
    const present = exactIntegerText(event.present_damage, `${key} present damage`);
    const providerEntityUuid = Number(event.status?.source_entity_uuid);
    if (event.exact_build_locked !== true ||
      !String(event.rlog ?? "") || !String(event.session_id ?? "") ||
      !Number.isSafeInteger(Number(event.run_ordinal)) || Number(event.run_ordinal) <= 0 ||
      !Number.isSafeInteger(Number(event.damage_source_entity_uuid)) ||
      Number(event.damage_source_entity_uuid) <= 0 ||
      !Number.isSafeInteger(Number(event.damage_target_entity_uuid)) ||
      Number(event.damage_target_entity_uuid) <= 0 ||
      !Number.isSafeInteger(Number(event.ability_id)) || Number(event.ability_id) <= 0 ||
      Number(event.status?.effect_id) !== effectId ||
      !Number.isSafeInteger(providerEntityUuid) || providerEntityUuid <= 0 ||
      Number(event.provider_player_identity?.provider_entity_uuid) !== providerEntityUuid ||
      !/^\d+$/.test(String(event.provider_player_identity?.character_id ?? "")) ||
      !/^sha256:[0-9a-f]{64}$/.test(String(event.normalized_packet_input_sha256 ?? "")) ||
      !Array.isArray(event.absent_sequences) || event.absent_sequences.length === 0 ||
      event.absent_sequences.some((sequence) => !Number.isSafeInteger(Number(sequence)) || Number(sequence) <= 0) ||
      !Array.isArray(event.present_sequences) || event.present_sequences.length === 0 ||
      event.present_sequences.some((sequence) => !Number.isSafeInteger(Number(sequence)) || Number(sequence) <= 0) ||
      absent + delta !== present ||
      event.arithmetic_equation !== `${absent} + ${delta} = ${present}` ||
      event.arithmetic_conservation_holds !== true ||
      event.exact_recorded_inputs_controlled !== true ||
      event.provider_player_identity_proven !== true ||
      event.causal_provider_contribution_proven !== false) {
      throw new Error(`Status-effect integer transform constraints ${key} have an invalid event-local conservation row`);
    }
  }
  return structuredClone(proof);
}

function exactIntegerText(value, label) {
  const text = String(value ?? "");
  if (!/^-?\d+$/.test(text)) throw new Error(`${label} must be an exact integer string`);
  return BigInt(text);
}

function validateNearControlledExhaustion(document, effectId, key) {
  const proof = document.near_controlled_exhaustion;
  const summary = proof?.summary;
  const interpretation = proof?.interpretation;
  const input = proof?.input;
  if (document.policy
      ?.near_controlled_target_diagnostics_never_grant_formula_or_runtime_authority !== true ||
    document.interpretation?.near_controlled_diagnostic_formula_authority !== false ||
    !String(input?.path ?? "") || !Number.isSafeInteger(Number(input?.bytes)) || Number(input.bytes) <= 0 ||
    !/^[0-9a-f]{64}$/.test(String(input?.sha256 ?? "")) ||
    !/^[0-9a-f]{64}$/.test(String(proof?.content_sha256 ?? "")) ||
    !Number.isSafeInteger(Number(summary?.matching_capture_runs)) || summary.matching_capture_runs <= 0 ||
    !Number.isSafeInteger(Number(summary?.samples)) || summary.samples <= 0 ||
    summary?.exact_divergent_capture_runs !== 1 ||
    summary?.near_controlled_target_divergent_pairs !== 0 ||
    summary?.equal_output_status_bundle_examples !== 1 ||
    summary?.near_controlled_target_pairs !==
      summary?.near_controlled_target_divergent_pairs + summary?.near_controlled_target_equal_pairs ||
    interpretation?.independent_divergent_baseline_replication_proven !== false ||
    interpretation?.additional_near_controlled_divergent_replication_observed !== false ||
    interpretation?.equal_output_status_bundle_diagnostic_observed !== true ||
    interpretation?.equal_output_status_bundle_is_an_isolated_effect_zero_proof !== false ||
    interpretation?.target_current_hp_is_controlled_in_equal_output_bundle !== false ||
    interpretation?.candidate_status_is_isolated_in_equal_output_bundle !== false ||
    interpretation?.exact_transform_proven !== false ||
    interpretation?.operation_order_and_stacking_proven !== false ||
    interpretation?.runtime_integer_rounding_proven !== false ||
    interpretation?.canonical_party_conservation_proven !== false ||
    interpretation?.formula_authority !== false || interpretation?.runtime_authority !== false ||
    interpretation?.ui_authority !== false ||
    interpretation?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(interpretation?.next_required_evidence) ||
    interpretation.next_required_evidence.length === 0) {
    throw new Error(`Status-effect integer transform constraints ${key} have unsafe near-controlled exhaustion evidence`);
  }
  return structuredClone(proof);
}

function validateProviderFormulaContextSummary(document, key) {
  const summary = document.provider_formula_context_summary;
  if (document.policy?.third_party_provider_attributes_are_controlled_when_embedded !== true ||
    document.interpretation?.provider_formula_base_input_proven !== false ||
    ![4, 5, 6].includes(Number(summary?.counterfactual_schema_version)) ||
    summary?.formula_context_embedded_for_every_example !== true ||
    summary?.exact_provider_attribute_state_controlled_for_every_example !== true ||
    summary?.provider_attribute_state_observed_for_every_example !== true ||
    summary?.provider_formula_base_input_proven !== false ||
    !Array.isArray(summary.provider_entity_uuids) || summary.provider_entity_uuids.length === 0 ||
    summary.provider_entity_uuids.some((value) => !Number.isSafeInteger(value) || value <= 0) ||
    !Array.isArray(summary.missing_or_unproven_inputs) ||
    summary.missing_or_unproven_inputs.length === 0) {
    throw new Error(`Status-effect integer transform constraints ${key} have unsafe provider formula context`);
  }
  return structuredClone(summary);
}

function validateProviderFormulaInputCoverage(document, effectId, key) {
  const coverage = document.provider_formula_input_coverage;
  const summary = document.provider_formula_context_summary;
  const candidateInputs = coverage?.audited_candidate_formula_inputs;
  if (document.policy?.exact_provider_formula_input_coverage_is_build_locked_when_supplied !== true ||
    coverage?.exact_build_identity !== String(document.game_build) ||
    Number(coverage?.effect_id) !== effectId ||
    coverage?.exact_provider_and_input_identity_match !== true ||
    !coverage.inputs?.provider_ownership_proof ||
    !coverage.inputs?.provider_formula_input_coverage ||
    !Array.isArray(coverage.proven_provider_entity_uuids) ||
    coverage.proven_provider_entity_uuids.length === 0 ||
    coverage.proven_provider_entity_uuids.some((value) =>
      !Number.isSafeInteger(value) || value <= 0
    ) || !Array.isArray(coverage.capture_inputs) || coverage.capture_inputs.length === 0 ||
    coverage.capture_inputs.some((input) =>
      !String(input.rlog ?? "") || String(input.game_build ?? "") !== String(document.game_build) ||
      !Number.isSafeInteger(Number(input.bytes)) || Number(input.bytes) <= 0 ||
      !/^sha256:[0-9a-f]{64}$/.test(String(input.sha256 ?? "")) ||
      !Number.isSafeInteger(Number(input.canonical_events_scanned)) ||
      Number(input.canonical_events_scanned) <= 0 ||
      !Number.isSafeInteger(Number(input.selected_actor_attribute_events_scanned)) ||
      Number(input.selected_actor_attribute_events_scanned) <= 0
    ) || new Set(coverage.capture_inputs.map((input) => input.rlog)).size !==
      coverage.capture_inputs.length ||
    ![11330, 11340, 11940].every((attributeId) => {
      const input = Object.values(candidateInputs ?? {}).find((entry) =>
        Number(entry?.attribute_id) === attributeId
      );
      return input && Number.isSafeInteger(Number(input.observation_count)) &&
        Number(input.observation_count) >= 0;
    }) ||
    coverage.interpretation?.packet_absence_is_not_zero !== true ||
    coverage.interpretation?.unobserved_inputs_are_not_backfilled_or_derived !== true ||
    coverage.interpretation?.spheal_operator_contract_proven !== false ||
    coverage.interpretation?.effect_output_to_status_transform_binding_proven !== false ||
    coverage.interpretation?.provider_formula_base_input_proven !== false ||
    coverage.interpretation?.formula_authority !== false ||
    coverage.interpretation?.runtime_authority !== false ||
    coverage.interpretation?.provider_rdps_credit_allowed !== false ||
    summary?.matching_capture_provider_formula_input_coverage_supplied !== true ||
    summary?.matching_capture_provider_formula_input_coverage_proven !== true ||
    Number(summary?.matching_capture_count) !== coverage.capture_inputs.length ||
    Number(summary?.proven_effect_provider_count) !== coverage.proven_provider_entity_uuids.length ||
    Number(summary?.matching_capture_physical_attack_observation_count) !==
      Number(candidateInputs?.physical_attack?.observation_count) ||
    summary?.matching_capture_physical_attack_absent_for_every_proven_provider !==
      (Number(candidateInputs?.physical_attack?.observation_count) === 0)) {
    throw new Error(`Status-effect integer transform constraints ${key} have unsafe provider formula-input coverage`);
  }
  return structuredClone(coverage);
}

function validateSpHealOperatorEvidence(document, effectId, key) {
  const evidence = document.spheal_operator_evidence;
  const summary = evidence?.summary;
  const contextSummary = document.provider_formula_context_summary;
  if (document.policy?.exact_spheal_operator_evidence_is_fail_closed_when_supplied !== true ||
    evidence?.exact_build_identity !== String(document.game_build) ||
    Number(evidence?.effect_id) !== effectId || !evidence.proof ||
    !Array.isArray(evidence.exact_effect_static_rows) ||
    evidence.exact_effect_static_rows.length === 0 ||
    !Array.isArray(evidence.input_rlogs) || evidence.input_rlogs.length === 0 ||
    evidence.input_rlogs.some((input) =>
      String(input.game_build ?? "") !== String(document.game_build) ||
      !String(input.path ?? "") || !Number.isSafeInteger(Number(input.bytes)) ||
      Number(input.bytes) <= 0 || !/^sha256:[0-9a-f]{64}$/.test(String(input.sha256 ?? ""))
    ) || new Set(evidence.input_rlogs.map((input) => input.path)).size !== evidence.input_rlogs.length ||
    !Array.isArray(evidence.exact_effect_occurrence_rlogs) ||
    evidence.exact_effect_occurrence_rlogs.length !==
      Number(summary?.exact_effect_occurrence_proof_rlogs ?? -1) ||
    evidence.exact_effect_occurrence_rlogs.some((input) =>
      String(input.game_build ?? "") !== String(document.game_build) ||
      !String(input.path ?? "") || !Number.isSafeInteger(Number(input.bytes)) ||
      Number(input.bytes) <= 0 || !/^sha256:[0-9a-f]{64}$/.test(String(input.sha256 ?? ""))
    ) || new Set(evidence.exact_effect_occurrence_rlogs.map((input) => input.path)).size !==
      evidence.exact_effect_occurrence_rlogs.length ||
    summary?.exact_effect_output_packet_observed !== false ||
    !Number.isSafeInteger(Number(summary?.exact_effect_occurrence_proof_healing_events_scanned)) ||
    Number(summary.exact_effect_occurrence_proof_healing_events_scanned) < 0 ||
    summary?.exact_effect_occurrence_proof_selected_events !== 0 ||
    summary?.spheal_family_wide_single_hp_ratio_proven !== false ||
    summary?.exact_effect_spheal_coefficient_to_hp_basis_binding_proven !== false ||
    summary?.damage_script_identity_alone_proves_operator !== false ||
    summary?.exact_effect_operator_proven !== false ||
    evidence.interpretation?.exact_effect_output_occurrence_missing !== true ||
    evidence.interpretation?.exact_effect_output_absent_in_all_complete_matching_build_capture_inputs !== true ||
    evidence.interpretation?.heterogeneous_spheal_family_evidence !== true ||
    evidence.interpretation?.family_name_transfer_to_exact_effect_allowed !== false ||
    evidence.interpretation?.exact_effect_formula_authority !== false ||
    evidence.interpretation?.exact_effect_runtime_authority !== false ||
    evidence.interpretation?.provider_rdps_credit_allowed !== false ||
    evidence.formula_authority !== false || evidence.runtime_authority !== false ||
    evidence.provider_rdps_credit_allowed !== false ||
    contextSummary?.spheal_operator_evidence_supplied !== true ||
    contextSummary?.exact_effect_spheal_output_packet_observed !== false ||
    Number(contextSummary?.exact_effect_spheal_occurrence_proof_capture_count) !==
      evidence.exact_effect_occurrence_rlogs.length ||
    Number(contextSummary?.exact_effect_spheal_occurrence_proof_healing_events_scanned) !==
      Number(summary.exact_effect_occurrence_proof_healing_events_scanned) ||
    contextSummary?.exact_effect_spheal_output_absent_in_all_complete_matching_build_capture_inputs !== true ||
    contextSummary?.spheal_family_wide_single_hp_ratio_proven !== false ||
    contextSummary?.exact_effect_spheal_coefficient_to_hp_basis_binding_proven !== false) {
    throw new Error(`Status-effect integer transform constraints ${key} have unsafe SpHeal operator evidence`);
  }
  return structuredClone(evidence);
}

function validateStaticFormulaInputCandidates(document, effectId, key) {
  const evidence = document.static_formula_input_candidates;
  if (document.policy?.decoded_formula_inputs_are_candidates_not_status_transform_bindings !== true ||
    document.interpretation?.exact_static_status_transform_binding_proven !== false ||
    !evidence || !Array.isArray(evidence.rows) || evidence.rows.length === 0 ||
    evidence.interpretation?.exact_type_enum_output_link_proven !== true ||
    evidence.interpretation?.typed_current_build_inputs_preserved !== true ||
    evidence.interpretation?.coefficient_is_effect_output_formula_input_not_proven_status_modifier !== true ||
    evidence.interpretation?.incompatibility_with_hypothetical_post_output_delta_does_not_disprove_other_server_operations !== true ||
    evidence.interpretation?.exact_static_status_transform_binding_proven !== false ||
    evidence.interpretation?.formula_authority !== false ||
    evidence.interpretation?.runtime_authority !== false ||
    !Array.isArray(evidence.hypothetical_post_output_delta_compatibility)) {
    throw new Error(`Status-effect integer transform constraints ${key} have unsafe static formula evidence`);
  }
  if (document.schema_version >= 3 &&
    (evidence.exact_build_identity !== String(document.game_build) ||
      evidence.exact_build_table_hash_match_proven !== true ||
      evidence.selected_rows_match_semantic_surface !== true ||
      !evidence.inputs?.damage_attr_table ||
      !evidence.inputs?.exact_build_formula_surface)) {
    throw new Error(`Status-effect integer transform constraints ${key} lack exact-build static input binding`);
  }
  const damageAttrIds = new Set();
  for (const row of evidence.rows) {
    if (!Number.isSafeInteger(row.damage_attr_id) || row.damage_attr_id <= 0 ||
      row.type_enum !== effectId || damageAttrIds.has(row.damage_attr_id) ||
      !Array.isArray(row.coefficient_basis_points_by_stage) ||
      row.coefficient_basis_points_by_stage.some((value) => !Number.isSafeInteger(value)) ||
      !Array.isArray(row.fixed_parameter_by_level) ||
      row.fixed_parameter_by_level.some((value) => !Number.isSafeInteger(value))) {
      throw new Error(`Status-effect integer transform constraints ${key} have an invalid static DamageAttr row`);
    }
    damageAttrIds.add(row.damage_attr_id);
  }
  for (const candidate of evidence.hypothetical_post_output_delta_compatibility) {
    if (!Number.isSafeInteger(candidate.basis_points) ||
      !Array.isArray(candidate.compatible_rounding_modes) ||
      candidate.compatible_rounding_modes.some((mode) =>
        !["floor", "round_half_up", "ceil"].includes(mode))) {
      throw new Error(`Status-effect integer transform constraints ${key} have invalid static compatibility evidence`);
    }
  }
  return structuredClone(evidence);
}

function isValidTargetMitigationStatusConfounderExhaustion(exhaustion) {
  return Number(exhaustion?.matching_build_capture_proofs) === 24 &&
    Number(exhaustion?.matching_build_source_rlogs) === 26 &&
    Number(exhaustion?.damage_samples) === 735016 &&
    Number(exhaustion?.target_locus_observed_samples) === 3009 &&
    Number(exhaustion?.exact_target_locus_controlled_groups) === 0 &&
    exhaustion?.every_common_confounder_observed_at_target_locus === true &&
    exhaustion?.every_common_confounder_exactly_controlled_at_target_locus === false &&
    exhaustion?.common_status_confounders_eliminated === false;
}

function isValidRlogGapWindowAudit(audit) {
  return audit?.status === "exact-gap-bounded-lifecycles-found-counterfactual-unproven" &&
    Number(audit?.source_rlog_count) === 26 &&
    Number(audit?.canonical_event_count) === 6411565 &&
    Number(audit?.data_gap_count) === 16181 &&
    Number(audit?.rlogs_with_data_gaps) === 26 &&
    Number(audit?.complete_gap_bounded_lifecycle_count) === 39 &&
    Number(audit?.complete_windows_with_damage_count) === 39 &&
    Number(audit?.damage_events_while_active) === 2277 &&
    Number(audit?.lifecycles_cut_by_data_quality_boundary) === 51 &&
    Array.isArray(audit?.complete_gap_bounded_windows) &&
    audit.complete_gap_bounded_windows.length === 39 &&
    audit.complete_gap_bounded_windows.every((window) =>
      window?.gap_bounded === true &&
      Number(window?.damage_events_while_active) > 0 &&
      window?.controlled_counterfactual_pair_proven === false &&
      window?.formula_authority === false) &&
    audit?.exact_damage_projection_proven === false &&
    audit?.exact_operation_order_proven === false &&
    audit?.exact_integer_rounding_proven === false &&
    audit?.packet_conservation_proven === false &&
    audit?.formula_authority === false &&
    audit?.runtime_authority === false &&
    audit?.ui_display_authority === false &&
    audit?.provider_rdps_credit_allowed === false;
}

function isValidRlogTransitionCounterfactualAudit(audit) {
  return audit?.status === "transition-adjacent-local-search-no-exact-observed-input-control" &&
    Number(audit?.source_rlog_count) === 26 && Number(audit?.canonical_event_count) === 6411565 &&
    Number(audit?.data_gap_count) === 16181 && Number(audit?.damage_events) === 735016 &&
    Number(audit?.damage_events_with_selected_effect_active) === 5463 &&
    Number(audit?.opposite_state_recent_comparisons) === 47626 &&
    Number(audit?.same_normalized_damage_context_pairs) === 37 &&
    Number(audit?.same_context_and_observed_attribute_pairs) === 0 &&
    Number(audit?.exact_observed_input_candidate_pairs) === 0 &&
    Number(audit?.target_current_hp_excluded_candidate_pairs) === 0 &&
    Number(audit?.strict_controlled_counterfactual_pairs) === 0 &&
    audit?.remote_player_packet_dependency === false &&
    audit?.exact_damage_projection_proven === false && audit?.exact_operation_order_proven === false &&
    audit?.exact_integer_rounding_proven === false && audit?.packet_conservation_proven === false &&
    audit?.formula_authority === false && audit?.runtime_authority === false &&
    audit?.ui_display_authority === false && audit?.provider_rdps_credit_allowed === false;
}

function isValidRlogTransitionMismatchFrontier(audit) {
  return Number(audit?.same_context_pairs_with_only_target_current_hp_difference) === 0 &&
    Number(audit?.same_context_source_attribute_difference_counts?.[474]) === 37 &&
    Number(audit?.same_context_target_attribute_difference_counts?.[443]) === 37 &&
    Number(audit?.same_context_target_attribute_difference_counts?.[474]) === 37 &&
    Number(audit?.same_context_target_attribute_difference_counts?.[11310]) === 37 &&
    Object.keys(audit?.same_context_target_temporary_attribute_difference_counts ?? {}).length === 0;
}

function isValidRlogTransitionStagedResidualFrontier(audit) {
  const closest = audit?.closest_residual_pair;
  return Number(audit?.same_context_pairs_after_443_474_attribute_exclusion) === 0 &&
    Number(audit?.same_context_pairs_after_443_474_and_target_current_hp_exclusion) === 1 &&
    Number(audit
      ?.same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses) === 0 &&
    Number(audit?.minimum_residual_observed_state_dimensions_after_443_474_exclusion) === 6 &&
    Number(audit
      ?.minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion) === 5 &&
    closest?.rlog === "runtime-data/logs/monitor-1787003553387.run-0005.rlog" &&
    closest?.session_id === "monitor-1787003553387.run-0005" &&
    Number(closest?.segment_index) === 266 && Number(closest?.present_sequence) === 378384 &&
    Number(closest?.absent_sequence) === 378486 && Number(closest?.pair_gap_micros) === 169986 &&
    Number(closest?.ability_id) === 2031105 && Number(closest?.present_amount) === 308131 &&
    Number(closest?.absent_amount) === 308131 && closest?.present_normal_value === null &&
    closest?.absent_normal_value === null &&
    JSON.stringify(closest?.source_attribute_ids) === JSON.stringify([474]) &&
    JSON.stringify(closest?.target_attribute_ids) === JSON.stringify([443, 474, 11310]) &&
    JSON.stringify(closest?.source_status_effect_ids) === JSON.stringify([55342, 2207252]) &&
    JSON.stringify(closest?.target_status_effect_ids) === JSON.stringify([21432, 2203311, 2203521]) &&
    Number(closest?.residual_observed_state_dimensions_after_443_474_exclusion) === 6 &&
    Number(closest
      ?.residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion) === 5 &&
    closest?.source_target_attribute_snapshots_complete === false &&
    closest?.selected_provider_exact === true && closest?.segment_status_baseline_complete === false &&
    closest?.controlled_counterfactual_pair_proven === false && closest?.formula_authority === false;
}

function isValidLuckyPacketComponentReceipt(proof) {
  return proof?.status ===
      "same-build-lucky-component-observed-nonstandard-formula-semantics-open" &&
    Number(proof?.ledger_lucky_rows) === 11 && Number(proof?.packet_observed_rows) === 9 &&
    Number(proof?.unobserved_ledger_rows) === 2 && Number(proof?.packet_damage_results) === 125183 &&
    Number(proof?.explicit_lucky_value_exact_matches) === 125183 &&
    proof?.packet_component_conservation_proven === true &&
    Number(proof?.selected_row?.damage_attr_id) === 2203110503 &&
    proof?.selected_row?.lookup_key === "2031105:3" &&
    proof?.selected_row?.formula_family === "MAttackLucky" &&
    Number(proof?.selected_row?.packet_damage_results) === 7762 &&
    proof?.selected_row?.same_build_packet_occurrence_proven === true &&
    proof?.selected_row?.packet_component_identity === "canonical-amount-equals-lucky-value" &&
    proof?.selected_row?.nonstandard_formula_semantics_proven === false &&
    proof?.selected_row?.physical_defense_dependency_proven === false &&
    proof?.selected_row?.magic_defense_dependency_proven === false &&
    proof?.nonstandard_formula_semantics_proven === false &&
    proof?.physical_or_magic_mitigation_route_proven === false &&
    proof?.formula_authority === false && proof?.runtime_attribution_authority === false &&
    proof?.ui_display_authority === false && proof?.provider_rdps_credit_allowed === false;
}

function isValidMAttackLuckyMitigationReceipt(proof) {
  return proof?.status === "same-build-mattack-lucky-mitigation-axes-unobserved" &&
    JSON.stringify(proof?.ability_ids) === JSON.stringify([2031102, 2031105, 2031111]) &&
    Number(proof?.hit_event_id) === 3 && Number(proof?.selected_sample_count) === 41111 &&
    JSON.stringify(proof?.samples_by_ability_id) ===
      JSON.stringify({ "2031102": 31284, "2031105": 7762, "2031111": 2065 }) &&
    Number(proof?.physical_defense_axis_samples) === 0 &&
    Number(proof?.magic_defense_axis_samples) === 0 &&
    Number(proof?.controlled_mitigation_pairs) === 0 &&
    proof?.remote_player_packet_dependency === false &&
    proof?.absent_axes_are_zero_mitigation === false &&
    proof?.exact_target_mitigation_formula_proven === false &&
    proof?.exact_operation_order_and_integer_rounding_proven === false &&
    proof?.complete_status_baseline_proven === false &&
    proof?.packet_conservation_proven === false && proof?.formula_authority === false &&
    proof?.runtime_authority === false && proof?.ui_display_authority === false &&
    proof?.provider_rdps_credit_allowed === false;
}

function isValidAttackLuckyComponentRows(rows) {
  return JSON.stringify((rows ?? []).map((row) => [
    Number(row?.ability_id),
    Number(row?.hit_event_id),
    Number(row?.packet_damage_results),
  ])) === JSON.stringify([
    [2031101, 3, 30281],
    [2031103, 3, 35887],
    [2031104, 3, 14684],
    [2031107, 3, 874],
    [2031109, 3, 1692],
    [2031110, 3, 654],
  ]);
}

function isValidAttackLuckyMitigationReceipt(proof) {
  return proof?.status === "same-build-attack-lucky-mitigation-axes-unobserved" &&
    JSON.stringify(proof?.ability_ids) ===
      JSON.stringify([2031101, 2031103, 2031104, 2031107, 2031109, 2031110]) &&
    Number(proof?.hit_event_id) === 3 && Number(proof?.selected_sample_count) === 84072 &&
    JSON.stringify(proof?.samples_by_ability_id) === JSON.stringify({
      "2031101": 30281,
      "2031103": 35887,
      "2031104": 14684,
      "2031107": 874,
      "2031109": 1692,
      "2031110": 654,
    }) &&
    Number(proof?.physical_defense_axis_samples) === 0 &&
    Number(proof?.magic_defense_axis_samples) === 0 &&
    Number(proof?.controlled_mitigation_pairs) === 0 &&
    proof?.remote_player_packet_dependency === false &&
    proof?.absent_axes_are_zero_mitigation === false &&
    proof?.exact_target_mitigation_formula_proven === false &&
    proof?.exact_operation_order_and_integer_rounding_proven === false &&
    proof?.complete_status_baseline_proven === false &&
    proof?.packet_conservation_proven === false && proof?.formula_authority === false &&
    proof?.runtime_authority === false && proof?.ui_display_authority === false &&
    proof?.provider_rdps_credit_allowed === false;
}

function isValidLuckyParentMultiplierReceipt(proof) {
  return proof?.status ===
      "exact-current-build-lucky-parent-complete-obvious-multiplier-candidates-rejected" &&
    Number(proof?.source_rlog_count) === 26 && Number(proof?.lucky_ability_id) === 2031109 &&
    Number(proof?.lucky_hit_event_id) === 3 && Number(proof?.lucky_events) === 1692 &&
    String(proof?.lucky_observed_damage) === "59479454" &&
    Number(proof?.immediate_same_wire_parent_events) === 1692 &&
    Number(proof?.unresolved_parent_events) === 0 && Number(proof?.ambiguous_parent_events) === 0 &&
    Number(proof?.multiplier_candidate_events) === 1289 &&
    Number(proof?.multiplier_candidate_exact_matches) === 0 &&
    Number(proof?.tested_multiplier_formulas_rejected) === 2 &&
    proof?.remote_player_packet_dependency === false &&
    proof?.parent_relation_for_observed_subset_proven === true &&
    proof?.lucky_multiplier_formula_proven === false &&
    proof?.general_lucky_formula_semantics_proven === false &&
    proof?.formula_authority === false && proof?.runtime_authority === false &&
    proof?.ui_display_authority === false && proof?.provider_rdps_credit_allowed === false;
}

function isValidGroupedLuckyParentMultiplierReceipt(proof) {
  return proof?.status ===
      "exact-current-build-lucky-parent-complete-recorded-multiplier-candidates-rejected" &&
    Number(proof?.source_rlog_count) === 26 && Number(proof?.lucky_ability_id) === 2031109 &&
    Number(proof?.lucky_hit_event_id) === 3 && Number(proof?.lucky_events) === 1692 &&
    String(proof?.lucky_observed_damage) === "59479454" &&
    Number(proof?.immediate_same_wire_parent_events) === 1692 &&
    Number(proof?.unresolved_parent_events) === 0 && Number(proof?.ambiguous_parent_events) === 0 &&
    Number(proof?.multiplier_candidate_events) === 1289 &&
    Number(proof?.multiplier_candidate_exact_matches) === 0 &&
    Number(proof?.source_attack_candidate_events) === 1289 &&
    Number(proof?.source_attack_candidate_exact_matches) === 0 &&
    Number(proof?.source_magic_attack_candidate_events) === 0 &&
    Number(proof?.relation_groups) === 52 &&
    Number(proof?.source_attack_candidate_minimum_residual) === 1823 &&
    Number(proof?.source_attack_candidate_maximum_residual) === 84858 &&
    Number(proof?.tested_multiplier_formulas_with_observations_rejected) === 3 &&
    proof?.remote_player_packet_dependency === false &&
    proof?.parent_relation_for_observed_subset_proven === true &&
    proof?.lucky_multiplier_formula_proven === false &&
    proof?.general_lucky_formula_semantics_proven === false &&
    proof?.formula_authority === false && proof?.runtime_authority === false &&
    proof?.ui_display_authority === false && proof?.provider_rdps_credit_allowed === false;
}

function isValidExhaustiveLocalSourceAttributeLuckyReceipt(proof) {
  return proof?.status ===
      "exact-current-build-lucky-parent-complete-all-local-single-attribute-multiplier-candidates-rejected" &&
    Number(proof?.source_rlog_count) === 26 && Number(proof?.lucky_ability_id) === 2031109 &&
    Number(proof?.lucky_hit_event_id) === 3 && Number(proof?.lucky_events) === 1692 &&
    String(proof?.lucky_observed_damage) === "59479454" &&
    Number(proof?.immediate_same_wire_parent_events) === 1692 &&
    Number(proof?.unresolved_parent_events) === 0 && Number(proof?.ambiguous_parent_events) === 0 &&
    Number(proof?.multiplier_candidate_events) === 1289 &&
    Number(proof?.multiplier_candidate_exact_matches) === 0 &&
    Number(proof?.source_attack_candidate_events) === 1289 &&
    Number(proof?.source_attack_candidate_exact_matches) === 0 &&
    Number(proof?.source_magic_attack_candidate_events) === 0 &&
    Number(proof?.relation_groups) === 52 &&
    Number(proof?.source_attack_candidate_minimum_residual) === 1823 &&
    Number(proof?.source_attack_candidate_maximum_residual) === 84858 &&
    Number(proof?.tested_multiplier_formulas_with_observations_rejected) === 3 &&
    Number(proof?.source_attribute_candidate_events) === 1289 &&
    Number(proof?.source_attribute_candidate_pairs) === 177361 &&
    Number(proof?.source_attribute_candidate_ids) === 224 &&
    Number(proof?.source_attribute_full_coverage_ids) === 67 &&
    Number(proof?.source_attribute_varying_ids) === 164 &&
    Number(proof?.source_attribute_within_relation_group_varying_ids) === 163 &&
    Number(proof?.source_attribute_candidate_exact_matches) === 0 &&
    proof?.simple_local_single_attribute_candidate_family_exhausted === true &&
    proof?.remote_player_packet_dependency === false &&
    proof?.parent_relation_for_observed_subset_proven === true &&
    proof?.lucky_multiplier_formula_proven === false &&
    proof?.general_lucky_formula_semantics_proven === false &&
    proof?.formula_authority === false && proof?.runtime_authority === false &&
    proof?.ui_display_authority === false && proof?.provider_rdps_credit_allowed === false;
}

function isValidRlogOpaqueAttributeAudit(audit) {
  return audit?.status ===
      "opaque-attributes-443-474-structurally-characterized-semantic-exclusion-unproven" &&
    Number(audit?.source_rlog_count) === 26 && Number(audit?.canonical_event_count) === 6411565 &&
    Number(audit?.reset_boundary_count) === 16247 &&
    audit?.remote_player_packet_dependency === false &&
    Number(audit?.attribute_443?.observation_count) === 71036 &&
    Number(audit?.attribute_443?.scalar_shape_observation_count) === 71036 &&
    Number(audit?.attribute_443?.most_common_signed_prior_delta) === -22 &&
    audit?.attribute_443?.semantic_identity_proven === false &&
    audit?.attribute_443?.safe_to_exclude_from_counterfactual_matching === false &&
    Number(audit?.attribute_474?.observation_count) === 266216 &&
    Number(audit?.attribute_474?.pair_collection_shape_observation_count) === 266216 &&
    Number(audit?.attribute_474?.pair_entry_count) === 1502529 &&
    Number(audit?.attribute_474?.pair_entries_matching_session_entities) === 1501983 &&
    audit?.attribute_474?.semantic_identity_proven === false &&
    audit?.attribute_474?.safe_to_exclude_from_counterfactual_matching === false &&
    audit?.formula_input_semantics_proven === false &&
    audit?.damage_consequence_semantics_proven === false &&
    audit?.safe_to_exclude_from_counterfactual_matching === false &&
    audit?.formula_authority === false && audit?.runtime_authority === false &&
    audit?.ui_display_authority === false && audit?.provider_rdps_credit_allowed === false;
}

function isValidSourceStatusConfounderRouteAudit(audit) {
  return audit?.status === "healing-action-route-proven-status-damage-neutrality-unproven" &&
    Number(audit?.effect_id) === 55342 && Number(audit?.linked_action_id) === 25534201 &&
    Number(audit?.packet_damage_results) === 0 && Number(audit?.packet_healing_results) === 22320 &&
    Number(audit?.same_context_source_status_difference_count) === 33 &&
    Number(audit?.strict_controlled_counterfactual_pairs) === 0 &&
    audit?.remote_player_packet_acquisition_required === false &&
    audit?.status_modifier_damage_neutrality_proven === false &&
    audit?.may_exclude_from_counterfactual_matching === false &&
    audit?.formula_authority === false && audit?.runtime_authority === false &&
    audit?.ui_display_authority === false && audit?.provider_rdps_credit_allowed === false;
}

function isValidSourceStatusLocalObservableAudit(audit) {
  return audit?.status ===
      "local-external-stat-transfer-subset-observed-general-formula-unproven" &&
    Number(audit?.effect_701010_windows) === 62935 &&
    Number(audit?.effect_701010_unresolved_cross_actor_windows) === 62935 &&
    Number(audit?.effect_2207252_external_player_windows) === 12948 &&
    Number(audit?.effect_2207252_exact_agility_delta_occurrences) === 48 &&
    Number(audit?.effect_2207252_exact_agility_delta_independent_runs) === 16 &&
    Number(audit?.remote_provider_attribute_context_examples) === 0 &&
    audit?.remote_player_attribute_acquisition_required === false &&
    audit?.current_snapshot_substitution_allowed === false &&
    audit?.general_transfer_percent_proven === false &&
    audit?.integer_rounding_proven === false &&
    audit?.exact_damage_projection_proven === false &&
    audit?.both_effects_remain_counterfactual_confounders === true &&
    audit?.formula_authority === false && audit?.runtime_authority === false &&
    audit?.ui_display_authority === false && audit?.provider_rdps_credit_allowed === false;
}

function isValidTargetEffectFormulaProof(proof) {
  return proof?.status === "gap-bounded-target-effect-formula-input-absent" &&
    Number(proof?.source_rlog_count) === 26 && Number(proof?.exact_effect_id) === 2110092 &&
    Number(proof?.complete_gap_bounded_lifecycles) === 39 &&
    Number(proof?.gap_audited_damage_window_memberships) === 2277 &&
    Number(proof?.gap_matched_unique_damage_events) === 2277 &&
    Number(proof?.formula_samples) === 2211 &&
    Number(proof?.gap_rows_excluded_by_wire_start_status) === 66 &&
    Number(proof?.source_physical_attack_samples) === 912 &&
    Number(proof?.target_physical_defense_samples) === 0 &&
    proof?.exact_armor_to_damage_equation_proven === false &&
    proof?.exact_operation_order_proven === false && proof?.exact_integer_rounding_proven === false &&
    proof?.packet_conservation_proven === false && proof?.formula_authority === false &&
    proof?.runtime_authority === false && proof?.ui_display_authority === false &&
    proof?.provider_rdps_credit_allowed === false;
}

function isValidTargetDefenseTransformBoundary(boundary) {
  return boundary?.status ===
      "exact-current-season-character-sheet-defense-transform-combat-stage-unbound" &&
    Number(boundary?.current_season_id) === 3 && boundary?.table === "FightAttrTranTable" &&
    boundary?.field === "DefPara" &&
    stableStringify(boundary?.parameters) === stableStringify([22000, 1, 1, 0, 0, 0, 0]) &&
    boundary?.exact_current_season_expression === "100 * raw / (raw + 22000)" &&
    Number(boundary?.exact_current_season_curve_constant) === 22000 &&
    boundary?.character_sheet_row_selection_proven === true &&
    boundary?.character_sheet_operation_order_proven === true &&
    boundary?.character_sheet_underlying_rounding === "none" &&
    boundary?.combat_stage_binding_proven === false &&
    boundary?.effect_reduces_raw_defense_before_transform_proven === false &&
    boundary?.server_integer_rounding_proven === false &&
    boundary?.packet_conservation_proven === false && boundary?.formula_authority === false &&
    boundary?.runtime_authority === false && boundary?.ui_rdps_display_authority === false &&
    boundary?.provider_rdps_credit_allowed === false;
}

function buildComponentScalarProofIndex(documents, proofPaths, build) {
  const index = new Map();
  for (let proofIndex = 0; proofIndex < documents.length; proofIndex += 1) {
    const proof = documents[proofIndex];
    const hasProviderOwnershipGapWorklist = Number(proof?.schema_version) >= 2;
    const hasTargetMitigationOfflineExhaustion = Number(proof?.schema_version) >= 3;
    const hasTargetMitigationAcquisitionWorklist = Number(proof?.schema_version) >= 4;
    const hasTargetMitigationNearPairCandidate = Number(proof?.schema_version) >= 5;
    const hasTargetMitigationStatusConfounderExhaustion = Number(proof?.schema_version) >= 6;
    const hasForwardStatusInstanceOwnership = Number(proof?.schema_version) >= 7;
    const hasSameWirePacketOwnership = Number(proof?.schema_version) >= 8;
    const hasSameAxisStatusEvidence = Number(proof?.schema_version) >= 9;
    const hasCounterfactualDiscriminants = Number(proof?.schema_version) >= 10;
    const hasTargetStatusActionRouteAudit = Number(proof?.schema_version) >= 11;
    const hasTargetDefensePercentLifecycleProof = Number(proof?.schema_version) >= 12;
    const hasRawPercentLifecycleProof = Number(proof?.schema_version) >= 13;
    const hasFightAttributeScopeProof = Number(proof?.schema_version) >= 14;
    const hasCritCoTransitionProof = Number(proof?.schema_version) >= 15;
    const hasTargetDefenseStatusDiagnosticRollup = Number(proof?.schema_version) >= 16;
    const hasTargetMitigationActorSceneExhaustion = Number(proof?.schema_version) >= 17;
    const hasRlogGapWindowAudit = Number(proof?.schema_version) >= 18;
    const hasRlogTransitionCounterfactualAudit = Number(proof?.schema_version) >= 19;
    const hasRlogTransitionMismatchFrontier = Number(proof?.schema_version) >= 20;
    const hasRlogOpaqueAttributeAudit = Number(proof?.schema_version) >= 21;
    const hasSourceStatusConfounderRouteAudit = Number(proof?.schema_version) >= 22;
    const hasSourceStatusLocalObservableAudit = Number(proof?.schema_version) >= 23;
    const hasTargetEffectFormulaProof = Number(proof?.schema_version) >= 24;
    const hasPacketFormulaIdentity = Number(proof?.schema_version) >= 25;
    const hasSameInputStatusInvariance = Number(proof?.schema_version) >= 26;
    const hasRlogTransitionStagedResidualFrontier = Number(proof?.schema_version) >= 27;
    const hasLuckyPacketComponentProof = Number(proof?.schema_version) >= 28;
    const hasMAttackLuckyMitigationDiagnostic = Number(proof?.schema_version) >= 29;
    const hasAttackLuckyMitigationDiagnostic = Number(proof?.schema_version) >= 30;
    const hasLuckyParentMultiplierProof = Number(proof?.schema_version) >= 31;
    const hasGroupedLuckyRelationProof = Number(proof?.schema_version) >= 32;
    const hasExhaustiveLocalSourceAttributeProof = Number(proof?.schema_version) >= 33;
    const hasTargetDefenseTransformBoundary = Number(proof?.schema_version) >= 34;
    if (![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34].includes(Number(proof?.schema_version)) ||
      proof?.generated_by !== "tools/bpsr-blade-sweep-scalar-proof.mjs" ||
      proof?.content_sha256 !== orderedContentHash(proof)) {
      throw new Error("Unsupported component static scalar proof schema, generator, or content hash");
    }
    requireBuild(proof.game_build, build, "component static scalar proof");
    const effectId = Number(proof.effect_id);
    if (!Number.isSafeInteger(effectId) || effectId <= 0 || index.has(effectId) ||
      proof.policy?.exact_numeric_ids_and_build_are_authoritative !== true ||
      proof.policy?.exact_input_hashes_are_embedded !== true ||
      proof.policy?.exact_static_scalar_does_not_prove_armor_to_damage_equation !== true ||
      proof.policy?.provider_ownership_must_be_packet_proven_for_every_lifecycle_event !== true ||
      proof.policy?.unresolved_provider_events_are_preserved !== true ||
      (hasTargetMitigationOfflineExhaustion &&
        proof.policy?.aggregate_offline_exhaustion_is_not_combat_formula_proof !== true) ||
      (hasTargetMitigationAcquisitionWorklist &&
        proof.policy?.target_status_relaxed_near_pairs_are_not_combat_formula_proof !== true) ||
      (hasTargetMitigationNearPairCandidate &&
        proof.policy?.status_confounded_integer_candidate_compatibility_is_not_combat_formula_proof !== true) ||
      (hasSameAxisStatusEvidence &&
        proof.policy
          ?.structurally_unobservable_remote_player_packets_are_not_formula_acquisition_requirements !== true) ||
      (hasCounterfactualDiscriminants &&
        proof.policy
          ?.candidate_counterfactual_discriminants_never_grant_formula_or_ui_authority !== true) ||
      (hasTargetStatusActionRouteAudit &&
        proof.policy
          ?.produced_action_routes_do_not_prove_status_modifier_damage_neutrality !== true) ||
      (hasTargetDefensePercentLifecycleProof &&
        proof.policy
          ?.exact_defense_stat_formula_does_not_prove_target_defense_to_damage_projection !== true) ||
      (hasRawPercentLifecycleProof &&
        proof.policy
          ?.defense_final_only_observations_are_preserved_without_claiming_raw_percent_packet_visibility !== true) ||
      (hasFightAttributeScopeProof &&
        proof.policy
          ?.complete_observed_fight_attribute_scope_does_not_exclude_hidden_damage_logic !== true) ||
      (hasCritCoTransitionProof &&
        proof.policy
          ?.sparse_crit_co_updates_do_not_establish_an_unconditional_secondary_component !== true) ||
      (hasTargetDefenseStatusDiagnosticRollup &&
        proof.policy
          ?.exhaustive_local_status_diagnostics_do_not_make_confounded_near_pairs_authoritative !== true) ||
      (hasTargetMitigationActorSceneExhaustion &&
        proof.policy
          ?.actor_scene_cross_capture_exhaustion_does_not_make_actor_shape_formula_authoritative !== true) ||
      (hasRlogGapWindowAudit &&
        proof.policy
          ?.complete_gap_bounded_lifecycle_windows_do_not_make_counterfactual_formula_authority !== true) ||
      (hasRlogTransitionCounterfactualAudit &&
        proof.policy
          ?.transition_adjacent_candidate_search_never_grants_counterfactual_formula_authority !== true) ||
      (hasRlogOpaqueAttributeAudit &&
        proof.policy
          ?.opaque_attribute_wire_shape_and_timing_never_grant_semantic_exclusion !== true) ||
      (hasSourceStatusConfounderRouteAudit &&
        proof.policy
          ?.healing_only_source_action_does_not_grant_status_confounder_exclusion !== true) ||
      (hasSourceStatusLocalObservableAudit &&
        proof.policy
          ?.locally_observed_dynamic_recipient_deltas_do_not_invent_remote_provider_inputs !== true) ||
      (hasTargetEffectFormulaProof &&
        proof.policy
          ?.gap_bounded_formula_rows_without_target_defense_do_not_prove_the_mitigation_curve !== true) ||
      (hasPacketFormulaIdentity &&
        proof.policy
          ?.exact_packet_component_and_coefficient_identity_do_not_prove_defense_stage_order !== true) ||
      (hasSameInputStatusInvariance &&
        proof.policy
          ?.same_input_status_invariance_does_not_remove_common_target_status_confounders !== true) ||
      (hasRlogTransitionStagedResidualFrontier &&
        proof.policy
          ?.attribute_443_474_and_target_current_hp_exclusion_is_diagnostic_only !== true) ||
      (hasLuckyPacketComponentProof &&
        proof.policy
          ?.lucky_packet_component_identity_does_not_prove_defense_dependency_or_formula_semantics !== true) ||
      (hasMAttackLuckyMitigationDiagnostic &&
        proof.policy
          ?.absent_observed_mitigation_axes_are_not_zero_mitigation_or_formula_proof !== true) ||
      (hasAttackLuckyMitigationDiagnostic &&
        proof.policy
          ?.both_lucky_families_require_observed_mitigation_inputs_before_route_selection !== true) ||
      (hasLuckyParentMultiplierProof &&
        proof.policy
          ?.complete_observed_lucky_parent_binding_does_not_invent_multiplier_formula_semantics !== true) ||
      (hasExhaustiveLocalSourceAttributeProof &&
        proof.policy
          ?.complete_local_source_attribute_candidate_exhaustion_does_not_invent_multi_input_formula_semantics !== true) ||
      (hasTargetDefenseTransformBoundary &&
        proof.policy?.exact_character_sheet_transform_does_not_prove_combat_stage_binding !== true) ||
      proof.policy?.formula_authority !== false || proof.policy?.runtime_authority !== false ||
      proof.policy?.provider_rdps_credit_allowed !== false ||
      proof.build_identity?.exhaustive_source_manifest_complete !== true ||
      proof.build_identity?.exact_static_table_hash_binding_proven !== true ||
      proof.build_identity?.decoded_table_bindings?.length !== 4 ||
      proof.static_scalar?.exact_static_scalar_proven !== true ||
      proof.static_scalar?.ladders_exactly_equal !== true ||
      proof.runtime_binding?.exact_effect_item_tier_target_binding_observed !== true ||
      Number(proof.summary?.observed_runtime_tier) !== 5 ||
      Number(proof.summary?.observed_runtime_armor_penetration_basis_points) !== 650 ||
      Number(proof.summary?.observed_runtime_armor_penetration_percent) !== 6.5 ||
      !Number.isSafeInteger(Number(proof.summary?.unresolved_provider_status_events)) ||
      (hasSameWirePacketOwnership
        ? Number(proof.summary?.unresolved_provider_status_events) !== 0
        : Number(proof.summary?.unresolved_provider_status_events) <= 0) ||
      Number(proof.summary?.target_mitigation_damage_samples) <= 0 ||
      Number(proof.summary?.target_mitigation_audited_axis_samples) <= 0 ||
      Number(proof.summary?.target_mitigation_controlled_groups) !== 0 ||
      Number(proof.summary?.maximum_target_mitigation_peak_working_set_mib) <= 0 ||
      Number(proof.summary?.global_target_mitigation_damage_samples) <
        Number(proof.summary?.target_mitigation_damage_samples) ||
      Number(proof.summary?.global_target_mitigation_audited_axis_samples) <
        Number(proof.summary?.target_mitigation_audited_axis_samples) ||
      Number(proof.summary?.global_target_mitigation_controlled_groups) !== 0 ||
      proof.summary?.exact_provider_ownership_proven !== hasSameWirePacketOwnership ||
      proof.summary?.exact_damage_projection_proven !== false ||
      proof.summary?.packet_conservation_proven !== false ||
      proof.summary?.formula_authority !== false || proof.summary?.runtime_authority !== false ||
      proof.summary?.provider_rdps_credit_allowed !== false ||
      (hasProviderOwnershipGapWorklist &&
        ((hasSameWirePacketOwnership
          ? proof.provider_ownership_gap_worklist?.status !== "exact-provider-ownership-proven"
          : proof.provider_ownership_gap_worklist?.status !== "exact-gap-inventory-acquisition-required") ||
          Number(proof.provider_ownership_gap_worklist?.unresolved_status_events) !==
            Number(proof.summary?.unresolved_provider_status_events) ||
          (hasSameWirePacketOwnership
            ? Number(proof.provider_ownership_gap_worklist?.gap_groups) !== 0
            : Number(proof.provider_ownership_gap_worklist?.gap_groups) <= 0) ||
          proof.provider_ownership_gap_worklist?.exact_provider_ownership_proven !==
            hasSameWirePacketOwnership ||
          proof.provider_ownership_gap_worklist?.formula_authority !== false ||
          proof.provider_ownership_gap_worklist?.runtime_authority !== false ||
          proof.provider_ownership_gap_worklist?.provider_rdps_credit_allowed !== false)) ||
      (hasForwardStatusInstanceOwnership &&
        (!Number.isSafeInteger(Number(
          proof.provider_ownership?.events_with_prior_status_instance_player_owner,
        )) ||
          Number(proof.provider_ownership.events_with_prior_status_instance_player_owner) <= 0 ||
          Number(proof.provider_ownership_gap_worklist
            ?.prior_status_instance_player_owned_status_events) !==
            Number(proof.provider_ownership.events_with_prior_status_instance_player_owner))) ||
      (hasSameWirePacketOwnership &&
        (!Number.isSafeInteger(Number(
          proof.provider_ownership?.events_with_same_wire_packet_player_owner,
        )) ||
          Number(proof.provider_ownership.events_with_same_wire_packet_player_owner) <= 0 ||
          Number(proof.provider_ownership_gap_worklist
            ?.same_wire_packet_player_owned_status_events) !==
            Number(proof.provider_ownership.events_with_same_wire_packet_player_owner) ||
          proof.provider_ownership?.exact_provider_ownership_for_every_event_proven !== true)) ||
      proof.counterfactual_projection?.exact_armor_to_damage_equation_proven !== false ||
      proof.counterfactual_projection?.exact_operation_order_proven !== false ||
      proof.counterfactual_projection?.exact_integer_rounding_proven !== false ||
      proof.counterfactual_projection?.packet_conservation_proven !== false ||
      proof.target_mitigation_evidence?.status !== "no-controlled-target-mitigation-pairs" ||
      Number(proof.target_mitigation_evidence?.damage_samples) !==
        Number(proof.summary?.target_mitigation_damage_samples) ||
      Number(proof.target_mitigation_evidence?.audited_axis_samples) !==
        Number(proof.summary?.target_mitigation_audited_axis_samples) ||
      Number(proof.target_mitigation_evidence?.controlled_groups) !== 0 ||
      Number(proof.target_mitigation_evidence?.maximum_measured_peak_working_set_bytes) <= 0 ||
      proof.target_mitigation_evidence?.exact_target_mitigation_formula_proven !== false ||
      proof.target_mitigation_evidence?.operation_order_and_integer_rounding_proven !== false ||
      proof.target_mitigation_evidence?.packet_conservation_proven !== false ||
      proof.target_mitigation_evidence?.formula_authority !== false ||
      proof.target_mitigation_evidence?.runtime_authority !== false ||
      proof.target_mitigation_evidence?.provider_rdps_credit_allowed !== false ||
      proof.global_target_mitigation_evidence?.status !== "no-controlled-target-mitigation-pairs" ||
      Number(proof.global_target_mitigation_evidence?.matching_build_source_rlogs) <= 0 ||
      Number(proof.global_target_mitigation_evidence?.damage_samples) <
        Number(proof.target_mitigation_evidence?.damage_samples) ||
      Number(proof.global_target_mitigation_evidence?.audited_axis_samples) <
        Number(proof.target_mitigation_evidence?.audited_axis_samples) ||
      Number(proof.global_target_mitigation_evidence?.controlled_groups) !== 0 ||
      proof.global_target_mitigation_evidence?.exact_target_mitigation_formula_proven !== false ||
      proof.global_target_mitigation_evidence?.operation_order_and_integer_rounding_proven !== false ||
      proof.global_target_mitigation_evidence?.packet_conservation_proven !== false ||
      proof.global_target_mitigation_evidence?.formula_authority !== false ||
      proof.global_target_mitigation_evidence?.runtime_authority !== false ||
      proof.global_target_mitigation_evidence?.provider_rdps_credit_allowed !== false ||
      (hasTargetMitigationOfflineExhaustion &&
        (proof.target_mitigation_offline_exhaustion?.status !==
          "exact-current-build-aggregate-offline-client-and-packet-search-exhausted-final-validation-required" ||
          Number(proof.target_mitigation_offline_exhaustion?.packet_capture_proofs) <= 0 ||
          Number(proof.target_mitigation_offline_exhaustion?.packet_source_rlogs) <
            Number(proof.target_mitigation_offline_exhaustion?.packet_capture_proofs) ||
          Number(proof.target_mitigation_offline_exhaustion?.packet_damage_samples) <
            Number(proof.global_target_mitigation_evidence?.damage_samples) ||
          Number(proof.target_mitigation_offline_exhaustion?.packet_audited_axis_samples) <
            Number(proof.global_target_mitigation_evidence?.audited_axis_samples) ||
          Number(proof.target_mitigation_offline_exhaustion
            ?.packet_samples_with_physical_or_refined_defense) <= 0 ||
          Number(proof.target_mitigation_offline_exhaustion?.packet_samples_with_magic_defense) < 0 ||
          Number(proof.target_mitigation_offline_exhaustion?.controlled_counterfactual_pairs) !== 0 ||
          Number(proof.target_mitigation_offline_exhaustion?.promoted_combat_formulas) !== 0 ||
          !Array.isArray(proof.target_mitigation_offline_exhaustion?.final_validation) ||
          proof.target_mitigation_offline_exhaustion.final_validation.length !== 2 ||
          proof.target_mitigation_offline_exhaustion?.exact_target_mitigation_formula_proven !== false ||
          proof.target_mitigation_offline_exhaustion
            ?.operation_order_and_integer_rounding_proven !== false ||
          proof.target_mitigation_offline_exhaustion?.packet_conservation_proven !== false ||
          proof.target_mitigation_offline_exhaustion?.formula_authority !== false ||
          proof.target_mitigation_offline_exhaustion?.runtime_authority !== false ||
          proof.target_mitigation_offline_exhaustion?.provider_rdps_credit_allowed !== false)) ||
      (hasTargetMitigationAcquisitionWorklist &&
        (proof.target_mitigation_acquisition_worklist?.status !==
          (hasSameAxisStatusEvidence
            ? "acquisition-required-strict-controls-status-damage-relevance-observed"
            : "acquisition-required-no-target-status-only-near-pair") ||
          Number(proof.target_mitigation_acquisition_worklist
            ?.matching_build_capture_diagnostics) <= 0 ||
          Number(proof.target_mitigation_acquisition_worklist?.damage_samples) !==
            Number(proof.target_mitigation_evidence?.damage_samples) ||
          Number(proof.target_mitigation_acquisition_worklist?.audited_axis_samples) !==
            Number(proof.target_mitigation_evidence?.audited_axis_samples) ||
          Number(proof.target_mitigation_acquisition_worklist?.strict_controlled_groups) !== 0 ||
          Number(proof.target_mitigation_acquisition_worklist
            ?.target_status_relaxed_distinct_axis_pairs) !== 0 ||
          Number(proof.target_mitigation_acquisition_worklist
            ?.pairs_with_effect_in_target_status_delta) !== 0 ||
          !Array.isArray(proof.target_mitigation_acquisition_worklist
            ?.acquisition_contract?.required_controls) ||
          proof.target_mitigation_acquisition_worklist.acquisition_contract
            .required_controls.length === 0 ||
          proof.target_mitigation_acquisition_worklist?.exact_target_mitigation_formula_proven !== false ||
          proof.target_mitigation_acquisition_worklist
            ?.operation_order_and_integer_rounding_proven !== false ||
          proof.target_mitigation_acquisition_worklist?.packet_conservation_proven !== false ||
          proof.target_mitigation_acquisition_worklist?.formula_authority !== false ||
          proof.target_mitigation_acquisition_worklist?.runtime_authority !== false ||
          proof.target_mitigation_acquisition_worklist?.provider_rdps_credit_allowed !== false)) ||
      (hasTargetMitigationNearPairCandidate &&
        (proof.target_mitigation_near_pair_candidate?.status !==
          "exact-integer-candidate-compatible-status-confounded" ||
          proof.target_mitigation_near_pair_candidate?.model_id !==
            "target-physical-armor-counterfactual" ||
          Number(proof.target_mitigation_near_pair_candidate?.transformed_curve_constant) !== 22000 ||
          Number(proof.target_mitigation_near_pair_candidate?.runtime_simple_curve_constant) !== 6500 ||
          Number(proof.target_mitigation_near_pair_candidate?.packet_near_pair_rows) !== 3 ||
          Number(proof.target_mitigation_near_pair_candidate?.transformed_curve_compatible_rows) !== 3 ||
          JSON.stringify(proof.target_mitigation_near_pair_candidate
            ?.transformed_curve_unique_shared_base_values) !== JSON.stringify(["107006"]) ||
          Number(proof.target_mitigation_near_pair_candidate
            ?.runtime_simple_curve_compatible_rows) !== 0 ||
          proof.target_mitigation_near_pair_candidate
            ?.selected_blade_sweep_effect_2110092_in_status_delta !== false ||
          proof.target_mitigation_near_pair_candidate?.exact_status_state_equal !== false ||
          proof.target_mitigation_near_pair_candidate
            ?.effect_2201452_damage_stage_exclusivity_proven !== false ||
          proof.target_mitigation_near_pair_candidate?.exact_target_mitigation_formula_proven !== false ||
          proof.target_mitigation_near_pair_candidate
            ?.operation_order_and_integer_rounding_proven !== false ||
          proof.target_mitigation_near_pair_candidate?.packet_conservation_proven !== false ||
          proof.target_mitigation_near_pair_candidate?.formula_authority !== false ||
          proof.target_mitigation_near_pair_candidate?.runtime_authority !== false ||
          proof.target_mitigation_near_pair_candidate?.provider_rdps_credit_allowed !== false)) ||
      (hasTargetMitigationStatusConfounderExhaustion &&
        !isValidTargetMitigationStatusConfounderExhaustion(
          proof.target_mitigation_near_pair_candidate?.confounder_counterfactual_exhaustion,
        )) ||
      (hasSameAxisStatusEvidence &&
        (proof.target_mitigation_acquisition_worklist
          ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
          Number(proof.target_mitigation_acquisition_worklist
            ?.global_same_axis_target_status_pairs) !== 5 ||
          Number(proof.target_mitigation_acquisition_worklist
            ?.global_same_axis_equal_output_pairs) !== 4 ||
          Number(proof.target_mitigation_acquisition_worklist
            ?.global_same_axis_divergent_output_pairs) !== 1 ||
          Number(proof.target_mitigation_near_pair_candidate?.same_axis_status_invariance
            ?.physical_defense_same_axis_status_pairs) !== 5 ||
          Number(proof.target_mitigation_near_pair_candidate?.same_axis_status_invariance
            ?.physical_defense_same_axis_equal_output_pairs) !== 4 ||
          Number(proof.target_mitigation_near_pair_candidate?.same_axis_status_invariance
            ?.physical_defense_same_axis_divergent_output_pairs) !== 1 ||
          proof.target_mitigation_near_pair_candidate?.same_axis_status_invariance
            ?.target_status_can_change_damage_outside_raw_defense !== true ||
          JSON.stringify(proof.target_mitigation_near_pair_candidate?.same_axis_status_invariance
            ?.candidate_status_effect_ids_without_same_axis_witness) !==
            JSON.stringify([55301, 2201452]))) ||
      (hasCounterfactualDiscriminants &&
        (proof.counterfactual_discriminants?.status !==
          "exact-candidate-discriminants-awaiting-controlled-packet-proof" ||
          Number(proof.counterfactual_discriminants?.armor_penetration_basis_points) !== 650 ||
          Number(proof.counterfactual_discriminants?.defense_curve_constant) !== 22000 ||
          !Array.isArray(proof.counterfactual_discriminants?.exact_discriminant_rows) ||
          proof.counterfactual_discriminants.exact_discriminant_rows.length !== 2 ||
          JSON.stringify(proof.counterfactual_discriminants
            ?.distinct_predicted_damage_with_effect) !==
            JSON.stringify([85530, 85533, 87122, 87125]) ||
          proof.counterfactual_discriminants?.acquisition_contract
            ?.remote_player_packet_dependency !== false ||
          proof.counterfactual_discriminants?.exact_damage_projection_proven !== false ||
          proof.counterfactual_discriminants?.exact_operation_order_proven !== false ||
          proof.counterfactual_discriminants?.exact_integer_rounding_proven !== false ||
          proof.counterfactual_discriminants?.packet_conservation_proven !== false ||
          proof.counterfactual_discriminants?.formula_authority !== false ||
          proof.counterfactual_discriminants?.runtime_authority !== false ||
          proof.counterfactual_discriminants?.ui_display_authority !== false ||
          proof.counterfactual_discriminants?.provider_rdps_credit_allowed !== false)) ||
      (hasPacketFormulaIdentity &&
        (proof.counterfactual_discriminants?.packet_formula_identity?.status !==
          "exact-build-packet-occurrence-static-route-and-coefficient-bound" ||
          Number(proof.counterfactual_discriminants?.packet_formula_identity?.ability_id) !== 823225 ||
          Number(proof.counterfactual_discriminants?.packet_formula_identity?.hit_event_id) !== 3 ||
          Number(proof.counterfactual_discriminants?.packet_formula_identity?.damage_attr_id) !== 282322503 ||
          JSON.stringify(proof.counterfactual_discriminants?.packet_formula_identity
            ?.pve_damage_ratio_basis_points) !== JSON.stringify([25000]) ||
          Number(proof.counterfactual_discriminants?.packet_formula_identity
            ?.packet_damage_results) !== 185 ||
          proof.counterfactual_discriminants?.packet_formula_identity
            ?.coefficient_to_pre_mitigation_base_formula_proven !== false ||
          proof.counterfactual_discriminants?.observed_baseline_curve?.status !==
            "three-distinct-defense-points-share-exact-integer-base-status-control-absent" ||
          Number(proof.counterfactual_discriminants?.observed_baseline_curve
            ?.exact_curve_compatible_rows) !== 22 ||
          Number(proof.counterfactual_discriminants?.observed_baseline_curve
            ?.preserved_status_confounded_rows) !== 1 ||
          Number(proof.counterfactual_discriminants?.observed_baseline_curve
            ?.unique_shared_nonnegative_base) !== 107006 ||
          proof.counterfactual_discriminants?.observed_baseline_curve
            ?.target_status_control_proven !== false ||
          Number(proof.summary?.exact_packet_damage_component_id) !== 282322503 ||
          Number(proof.summary?.exact_packet_component_coefficient_basis_points) !== 25000 ||
          Number(proof.summary?.actor_scene_curve_compatible_rows) !== 22 ||
          Number(proof.summary?.actor_scene_curve_distinct_defense_points) !== 3 ||
          Number(proof.summary?.actor_scene_curve_status_confounded_rows) !== 1)) ||
      (hasSameInputStatusInvariance &&
        (Number(proof.counterfactual_discriminants?.observed_baseline_curve
          ?.same_input_status_invariance?.compatible_target_status_state_ids) !== 20 ||
          Number(proof.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.common_effect_ids_across_all_compatible_rows?.length) !== 78 ||
          Number(proof.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.varying_effect_ids_across_all_compatible_rows?.length) !== 36 ||
          Number(proof.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.isolated_single_effect_toggle_count) !== 1 ||
          Number(proof.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.same_input_groups?.[1]
            ?.isolated_single_effect_toggle_receipts?.[0]?.effect_id) !== 2203182 ||
          proof.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.common_target_status_confounders_remain !== true ||
          proof.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.target_status_control_proven !== false ||
          Number(proof.summary?.actor_scene_compatible_target_status_states) !== 20 ||
          Number(proof.summary?.actor_scene_common_target_status_confounders) !== 78 ||
          Number(proof.summary?.actor_scene_varying_target_status_effects) !== 36 ||
          Number(proof.summary?.actor_scene_isolated_invariant_single_effect_toggles) !== 1)) ||
      (hasTargetStatusActionRouteAudit &&
        (proof.target_status_action_route_audit?.status !==
          "exact-produced-action-routes-audited-status-modifier-neutrality-unproven" ||
          Number(proof.target_status_action_route_audit?.audited_effects) !== 12 ||
          Number(proof.target_status_action_route_audit?.produced_damage_action_effects) !== 0 ||
          Number(proof.target_status_action_route_audit
            ?.produced_action_healing_only_effects) !== 3 ||
          Number(proof.target_status_action_route_audit?.no_produced_action_observed_effects) !== 9 ||
          Number(proof.target_status_action_route_audit?.effects_eliminated_as_damage_neutral) !== 0 ||
          JSON.stringify(proof.target_status_action_route_audit
            ?.candidate_near_pair_status_effects_without_same_axis_witness) !==
            JSON.stringify([55301, 2201452]) ||
          proof.target_status_action_route_audit?.status_modifier_damage_neutrality_proven !== false ||
          proof.target_status_action_route_audit?.target_status_confounders_eliminated !== false ||
          proof.target_status_action_route_audit?.formula_authority !== false ||
          proof.target_status_action_route_audit?.runtime_authority !== false ||
          proof.target_status_action_route_audit?.ui_display_authority !== false ||
          proof.target_status_action_route_audit?.provider_rdps_credit_allowed !== false)) ||
      (hasTargetDefensePercentLifecycleProof &&
        (proof.target_defense_percent_lifecycle_proof?.status !==
          "defense-stat-formula-proven-damage-counterfactual-unproven" ||
          Number(proof.target_defense_percent_lifecycle_proof?.effect_id) !== 2201452 ||
          Number(proof.target_defense_percent_lifecycle_proof?.attribute_id) !== 11350 ||
          Number(proof.target_defense_percent_lifecycle_proof?.percent_basis_points) !== 1000 ||
          Number(proof.target_defense_percent_lifecycle_proof?.exact_wire_occurrences) !== 51 ||
          Number(proof.target_defense_percent_lifecycle_proof?.application_occurrences) !== 30 ||
          Number(proof.target_defense_percent_lifecycle_proof?.removal_occurrences) !== 21 ||
          Number(proof.target_defense_percent_lifecycle_proof?.independent_sessions) !== 13 ||
          proof.target_defense_percent_lifecycle_proof
            ?.effect_2201452_exact_defense_axis_mechanism_proven !== true ||
          proof.target_defense_percent_lifecycle_proof
            ?.exact_target_defense_to_damage_formula_proven !== false ||
          proof.target_defense_percent_lifecycle_proof
            ?.effect_2201452_damage_stage_exclusivity_proven !== false ||
          proof.target_defense_percent_lifecycle_proof
            ?.hidden_additional_damage_stage_behavior_excluded !== false ||
          proof.target_defense_percent_lifecycle_proof?.formula_authority !== false ||
          proof.target_defense_percent_lifecycle_proof?.runtime_authority !== false ||
          proof.target_defense_percent_lifecycle_proof?.ui_display_authority !== false ||
          proof.target_defense_percent_lifecycle_proof?.provider_rdps_credit_allowed !== false ||
          proof.summary?.exact_effect_2201452_defense_stat_formula_proven !== true ||
          Number(proof.summary?.effect_2201452_exact_wire_transition_occurrences) !== 51 ||
          Number(proof.summary?.effect_2201452_exact_wire_independent_sessions) !== 13)) ||
      (hasRawPercentLifecycleProof &&
        (Number(proof.target_defense_percent_lifecycle_proof
          ?.packet_raw_percent_joined_occurrences) !== 47 ||
          Number(proof.target_defense_percent_lifecycle_proof
            ?.final_only_unresolved_occurrences) !== 4 ||
          Number(proof.target_defense_percent_lifecycle_proof
            ?.exact_family_input_transitions) !== 158 ||
          Number(proof.target_defense_percent_lifecycle_proof
            ?.nearest_rounding_residual_mismatches) !== 86 ||
          proof.target_defense_percent_lifecycle_proof
            ?.truncation_selected_over_round_to_nearest !== true ||
          proof.target_defense_percent_lifecycle_proof
            ?.raw_percent_identity_for_all_lifecycle_occurrences_proven !== false ||
          Number(proof.summary?.effect_2201452_packet_raw_percent_joined_occurrences) !== 47 ||
          Number(proof.summary?.effect_2201452_final_only_unresolved_occurrences) !== 4)) ||
      (hasFightAttributeScopeProof &&
        (proof.target_defense_fight_attribute_scope_proof?.status !==
          "complete-observed-fight-attribute-scope-hidden-damage-logic-unexcluded" ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.selected_fight_attribute_components) !== 906 ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.components_with_exact_single_effect_same_wire_correlations) !== 26 ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.proven_reversible_constant_components) !== 1 ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.unresolved_fight_attribute_components) !== 25 ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.only_proven_reversible_constant_attribute_id) !== 11354 ||
          JSON.stringify(proof.target_defense_fight_attribute_scope_proof
            ?.unresolved_one_direction_attribute_ids) !== JSON.stringify([11710, 11711, 11712]) ||
          proof.target_defense_fight_attribute_scope_proof
            ?.effect_is_defense_stat_only_across_observed_fight_attribute_components_proven !== false ||
          proof.target_defense_fight_attribute_scope_proof
            ?.hidden_damage_stage_behavior_excluded !== false ||
          proof.target_defense_fight_attribute_scope_proof?.formula_authority !== false ||
          proof.target_defense_fight_attribute_scope_proof?.runtime_authority !== false ||
          proof.target_defense_fight_attribute_scope_proof?.ui_display_authority !== false ||
          proof.target_defense_fight_attribute_scope_proof?.provider_rdps_credit_allowed !== false ||
          Number(proof.summary?.effect_2201452_selected_fight_attribute_components) !== 906 ||
          Number(proof.summary?.effect_2201452_proven_reversible_constant_components) !== 1 ||
          Number(proof.summary?.effect_2201452_unresolved_fight_attribute_components) !== 25)) ||
      (hasCritCoTransitionProof &&
        (Number(proof.target_defense_fight_attribute_scope_proof
          ?.raw_armor_presence_transitions) !== 47 ||
          Number(proof.target_defense_fight_attribute_scope_proof?.raw_armor_applications) !== 26 ||
          Number(proof.target_defense_fight_attribute_scope_proof?.raw_armor_removals) !== 21 ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.raw_crit_add_application_co_updates) !== 0 ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.raw_crit_add_removal_co_updates) !== 2 ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.raw_armor_transitions_without_raw_crit_co_update) !== 45 ||
          Number(proof.target_defense_fight_attribute_scope_proof
            ?.removal_only_raw_crit_add_delta) !== 50 ||
          proof.target_defense_fight_attribute_scope_proof
            ?.unconditional_fixed_negative_50_raw_crit_add_component_supported !== false ||
          proof.target_defense_fight_attribute_scope_proof
            ?.conditional_or_indirect_crit_behavior_excluded !== false ||
          Number(proof.summary
            ?.effect_2201452_raw_armor_transitions_without_raw_crit_co_update) !== 45)) ||
      (hasTargetDefenseStatusDiagnosticRollup &&
        (proof.target_defense_status_diagnostic_rollup?.status !==
          "exhaustive-local-status-diagnostic-search-no-independent-control" ||
          Number(proof.target_defense_status_diagnostic_rollup
            ?.matching_build_capture_diagnostics) !== 24 ||
          Number(proof.target_defense_status_diagnostic_rollup?.damage_samples) !== 735016 ||
          Number(proof.target_defense_status_diagnostic_rollup
            ?.physical_defense_unique_near_pairs) !== 3 ||
          Number(proof.target_defense_status_diagnostic_rollup
            ?.physical_defense_pairs_with_selected_effect_in_status_delta) !== 3 ||
          Number(proof.target_defense_status_diagnostic_rollup
            ?.physical_defense_same_axis_pairs_with_selected_effect_in_status_delta) !== 0 ||
          proof.target_defense_status_diagnostic_rollup
            ?.no_new_independent_local_control_was_found !== true ||
          proof.target_defense_status_diagnostic_rollup
            ?.remote_player_packet_acquisition_required !== false ||
          proof.target_defense_status_diagnostic_rollup?.formula_authority !== false ||
          proof.target_defense_status_diagnostic_rollup?.runtime_authority !== false ||
          proof.target_defense_status_diagnostic_rollup?.ui_display_authority !== false ||
          proof.target_defense_status_diagnostic_rollup?.provider_rdps_credit_allowed !== false ||
          Number(proof.summary?.effect_2201452_status_diagnostic_damage_samples) !== 735016 ||
          Number(proof.summary?.effect_2201452_physical_defense_near_pairs) !== 3 ||
          Number(proof.summary?.effect_2201452_near_pairs_with_effect_in_status_delta) !== 3 ||
          Number(proof.summary?.effect_2201452_same_axis_damage_witnesses) !== 0)) ||
      (hasTargetMitigationActorSceneExhaustion &&
        (proof.target_mitigation_actor_scene_exhaustion?.status !==
          "exact-local-actor-scene-exhausted-no-cross-capture-control" ||
          Number(proof.target_mitigation_actor_scene_exhaustion?.selected_ability_id) !== 823225 ||
          Number(proof.target_mitigation_actor_scene_exhaustion?.selected_ability_samples) !== 185 ||
          Number(proof.target_mitigation_actor_scene_exhaustion?.physical_defense_samples) !== 23 ||
          Number(proof.target_mitigation_actor_scene_exhaustion
            ?.physical_defense_samples_with_stable_target_actor_id) !== 0 ||
          Number(proof.target_mitigation_actor_scene_exhaustion?.cross_capture_actor_shape_pairs) !== 0 ||
          proof.target_mitigation_actor_scene_exhaustion
            ?.structurally_unavailable_remote_player_packets_are_not_required !== true ||
          proof.target_mitigation_actor_scene_exhaustion
            ?.missing_stable_remote_player_identity_is_preserved_not_synthesized !== true ||
          proof.target_mitigation_actor_scene_exhaustion?.formula_authority !== false ||
          proof.target_mitigation_actor_scene_exhaustion?.runtime_authority !== false ||
          proof.target_mitigation_actor_scene_exhaustion?.ui_display_authority !== false ||
          proof.target_mitigation_actor_scene_exhaustion?.provider_rdps_credit_allowed !== false ||
          Number(proof.summary?.actor_scene_selected_ability_samples) !== 185 ||
          Number(proof.summary?.actor_scene_physical_defense_samples) !== 23 ||
          Number(proof.summary?.actor_scene_cross_capture_pairs) !== 0 ||
          Number(proof.summary?.actor_scene_stable_target_actor_ids) !== 0)) ||
      (hasRlogGapWindowAudit &&
        (!isValidRlogGapWindowAudit(proof.rlog_gap_window_audit) ||
          Number(proof.summary?.effect_2110092_gap_bounded_complete_lifecycles) !== 39 ||
          Number(proof.summary?.effect_2110092_gap_bounded_windows_with_damage) !== 39 ||
          Number(proof.summary?.effect_2110092_gap_bounded_damage_events) !== 2277 ||
          Number(proof.summary?.effect_2110092_lifecycles_cut_by_data_quality_boundaries) !== 51)) ||
      (hasRlogTransitionCounterfactualAudit &&
        (!isValidRlogTransitionCounterfactualAudit(proof.rlog_transition_counterfactual_audit) ||
          Number(proof.summary?.effect_2110092_transition_opposite_state_comparisons) !== 47626 ||
          Number(proof.summary?.effect_2110092_transition_same_context_pairs) !== 37 ||
          Number(proof.summary?.effect_2110092_transition_exact_input_pairs) !== 0)) ||
      (hasRlogTransitionMismatchFrontier &&
        (!isValidRlogTransitionMismatchFrontier(proof.rlog_transition_counterfactual_audit) ||
          Number(proof.summary
            ?.effect_2110092_transition_only_current_hp_difference_pairs) !== 0)) ||
      (hasRlogTransitionStagedResidualFrontier &&
        (!isValidRlogTransitionStagedResidualFrontier(proof.rlog_transition_counterfactual_audit) ||
          Number(proof.summary?.effect_2110092_transition_pairs_after_443_474_exclusion) !== 0 ||
          Number(proof.summary
            ?.effect_2110092_transition_pairs_after_443_474_and_target_current_hp_exclusion) !== 1 ||
          Number(proof.summary
            ?.effect_2110092_transition_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses) !== 0 ||
          Number(proof.summary
            ?.effect_2110092_transition_minimum_residual_dimensions_after_diagnostic_exclusions) !== 5)) ||
      (hasLuckyPacketComponentProof &&
        (!isValidLuckyPacketComponentReceipt(proof.lucky_packet_component_proof) ||
          Number(proof.summary?.closest_transition_lucky_damage_attr_id) !== 2203110503 ||
          Number(proof.summary?.closest_transition_lucky_packet_damage_results) !== 7762 ||
          Number(proof.summary?.lucky_packet_component_damage_results) !== 125183 ||
          Number(proof.summary?.lucky_packet_component_exact_matches) !== 125183)) ||
      (hasMAttackLuckyMitigationDiagnostic &&
        (!isValidMAttackLuckyMitigationReceipt(proof.mattack_lucky_mitigation_diagnostic) ||
          Number(proof.mattack_lucky_mitigation_diagnostic?.samples_by_ability_id?.["2031105"]) !==
            Number(proof.lucky_packet_component_proof?.selected_row?.packet_damage_results) ||
          Number(proof.summary?.mattack_lucky_selected_damage_results) !== 41111 ||
          Number(proof.summary?.mattack_lucky_physical_defense_axis_samples) !== 0 ||
          Number(proof.summary?.mattack_lucky_magic_defense_axis_samples) !== 0)) ||
      (hasAttackLuckyMitigationDiagnostic &&
        (!isValidAttackLuckyComponentRows(
          proof.lucky_packet_component_proof?.attack_lucky_rows,
        ) ||
          !isValidAttackLuckyMitigationReceipt(proof.attack_lucky_mitigation_diagnostic) ||
          Number(proof.summary?.attack_lucky_selected_damage_results) !== 84072 ||
          Number(proof.summary?.attack_lucky_physical_defense_axis_samples) !== 0 ||
          Number(proof.summary?.attack_lucky_magic_defense_axis_samples) !== 0 ||
          Number(proof.summary?.both_lucky_families_selected_damage_results) !== 125183)) ||
      (hasLuckyParentMultiplierProof &&
        (!(hasExhaustiveLocalSourceAttributeProof
          ? isValidExhaustiveLocalSourceAttributeLuckyReceipt(proof.lucky_parent_multiplier_proof)
          : hasGroupedLuckyRelationProof
            ? isValidGroupedLuckyParentMultiplierReceipt(proof.lucky_parent_multiplier_proof)
            : isValidLuckyParentMultiplierReceipt(proof.lucky_parent_multiplier_proof)) ||
          Number(proof.summary?.lucky_parent_observed_events) !== 1692 ||
          Number(proof.summary?.lucky_parent_unresolved_events) !== 0 ||
          Number(proof.summary?.lucky_multiplier_candidate_events) !== 1289 ||
          Number(proof.summary?.lucky_multiplier_candidate_exact_matches) !== 0 ||
          (hasGroupedLuckyRelationProof &&
            (Number(proof.summary?.lucky_source_attack_candidate_exact_matches) !== 0 ||
              Number(proof.summary?.lucky_source_attack_relation_groups) !== 52)) ||
          (hasExhaustiveLocalSourceAttributeProof &&
            (Number(proof.summary?.lucky_source_attribute_candidate_events) !== 1289 ||
              Number(proof.summary?.lucky_source_attribute_candidate_pairs) !== 177361 ||
              Number(proof.summary?.lucky_source_attribute_candidate_ids) !== 224 ||
              Number(proof.summary?.lucky_source_attribute_candidate_exact_matches) !== 0)))) ||
      (hasRlogOpaqueAttributeAudit &&
        (!isValidRlogOpaqueAttributeAudit(proof.rlog_opaque_attribute_audit) ||
          Number(proof.summary?.opaque_attribute_443_observations) !== 71036 ||
          Number(proof.summary?.opaque_attribute_474_observations) !== 266216 ||
          Number(proof.summary?.opaque_attribute_474_pair_entries) !== 1502529 ||
          Number(proof.summary?.opaque_attribute_474_pair_entries_matching_session_entities) !==
            1501983)) ||
      (hasSourceStatusConfounderRouteAudit &&
        (!isValidSourceStatusConfounderRouteAudit(proof.source_status_confounder_route_audit) ||
          Number(proof.summary?.source_status_55342_packet_healing_results) !== 22320 ||
          Number(proof.summary?.source_status_55342_same_context_difference_count) !== 33)) ||
      (hasSourceStatusLocalObservableAudit &&
        (!isValidSourceStatusLocalObservableAudit(proof.source_status_local_observable_audit) ||
          Number(proof.summary?.source_status_2207252_external_player_windows) !== 12948 ||
          Number(proof.summary?.source_status_2207252_exact_agility_delta_occurrences) !== 48 ||
          Number(proof.summary
            ?.source_status_2207252_remote_provider_attribute_context_examples) !== 0)) ||
      (hasTargetEffectFormulaProof &&
        (!isValidTargetEffectFormulaProof(proof.target_effect_formula_proof) ||
          Number(proof.summary
            ?.effect_2110092_gap_bounded_wire_start_formula_samples) !== 2211 ||
          Number(proof.summary
            ?.effect_2110092_gap_bounded_target_physical_defense_samples) !== 0)) ||
      (hasTargetDefenseTransformBoundary &&
        (!isValidTargetDefenseTransformBoundary(proof.target_defense_transform_boundary) ||
          Number(proof.summary?.current_season_id) !== 3 ||
          Number(proof.summary?.exact_current_season_defense_curve_constant) !== 22000 ||
          proof.summary?.character_sheet_defense_transform_operation_order_proven !== true ||
          proof.summary?.combat_defense_transform_stage_binding_proven !== false))) {
      throw new Error("Component static scalar proof violates its fail-closed authority contract");
    }
    const inputs = Object.values(proof.inputs ?? {});
    if (inputs.length === 0 || inputs.some((input) =>
      !String(input?.path ?? "") || !Number.isSafeInteger(Number(input?.bytes)) || Number(input.bytes) <= 0 ||
      !/^[0-9a-f]{64}$/.test(String(input?.sha256 ?? "")))) {
      throw new Error("Component static scalar proof input provenance is incomplete");
    }
    const ownershipRlogs = proof.provider_ownership?.input_rlogs ?? [];
    const counterfactualRlogs = proof.counterfactual_projection?.input_rlogs ?? [];
    const rlogKey = (entry) =>
      `${path.basename(String(entry?.path ?? "")).toLowerCase()}|${Number(entry?.bytes)}|${String(entry?.sha256 ?? "")}`;
    if (ownershipRlogs.length === 0 ||
      stableStringify(ownershipRlogs.map(rlogKey).sort()) !== stableStringify(counterfactualRlogs.map(rlogKey).sort())) {
      throw new Error("Component static scalar proof ownership and counterfactual RLOG cohorts differ");
    }
    index.set(effectId, {
      effect_id: String(effectId),
      component_id: String(proof.exact_identity?.runtime_component_id ?? ""),
      proof: fileDescriptor(proofPaths[proofIndex]),
      proof_schema_version: Number(proof.schema_version),
      exact_static_scalar_proven: true,
      observed_runtime_tier: Number(proof.summary.observed_runtime_tier),
      observed_runtime_armor_penetration_basis_points:
        Number(proof.summary.observed_runtime_armor_penetration_basis_points),
      observed_runtime_armor_penetration_percent:
        Number(proof.summary.observed_runtime_armor_penetration_percent),
      unresolved_provider_status_events:
        Number(proof.summary.unresolved_provider_status_events),
      prior_status_instance_player_owned_status_events: hasForwardStatusInstanceOwnership
        ? Number(proof.provider_ownership.events_with_prior_status_instance_player_owner)
        : 0,
      same_wire_packet_player_owned_status_events: hasSameWirePacketOwnership
        ? Number(proof.provider_ownership.events_with_same_wire_packet_player_owner)
        : 0,
      provider_ownership_gap_worklist: hasProviderOwnershipGapWorklist
        ? structuredClone(proof.provider_ownership_gap_worklist)
        : null,
      target_mitigation_evidence: structuredClone(proof.target_mitigation_evidence),
      global_target_mitigation_evidence:
        structuredClone(proof.global_target_mitigation_evidence),
      target_mitigation_offline_exhaustion: hasTargetMitigationOfflineExhaustion
        ? structuredClone(proof.target_mitigation_offline_exhaustion)
        : null,
      target_mitigation_acquisition_worklist: hasTargetMitigationAcquisitionWorklist
        ? structuredClone(proof.target_mitigation_acquisition_worklist)
        : null,
      target_mitigation_near_pair_candidate: hasTargetMitigationNearPairCandidate
        ? structuredClone(proof.target_mitigation_near_pair_candidate)
        : null,
      target_defense_transform_boundary: hasTargetDefenseTransformBoundary
        ? structuredClone(proof.target_defense_transform_boundary)
        : null,
      counterfactual_discriminants: hasCounterfactualDiscriminants
        ? structuredClone(proof.counterfactual_discriminants)
        : null,
      target_status_action_route_audit: hasTargetStatusActionRouteAudit
        ? structuredClone(proof.target_status_action_route_audit)
        : null,
      target_defense_percent_lifecycle_proof: hasTargetDefensePercentLifecycleProof
        ? structuredClone(proof.target_defense_percent_lifecycle_proof)
        : null,
      target_defense_fight_attribute_scope_proof: hasFightAttributeScopeProof
        ? structuredClone(proof.target_defense_fight_attribute_scope_proof)
        : null,
      target_defense_status_diagnostic_rollup: hasTargetDefenseStatusDiagnosticRollup
        ? structuredClone(proof.target_defense_status_diagnostic_rollup)
        : null,
      target_mitigation_actor_scene_exhaustion: hasTargetMitigationActorSceneExhaustion
        ? structuredClone(proof.target_mitigation_actor_scene_exhaustion)
        : null,
      rlog_gap_window_audit: hasRlogGapWindowAudit
        ? structuredClone(proof.rlog_gap_window_audit)
        : null,
      rlog_transition_counterfactual_audit: hasRlogTransitionCounterfactualAudit
        ? structuredClone(proof.rlog_transition_counterfactual_audit)
        : null,
      rlog_opaque_attribute_audit: hasRlogOpaqueAttributeAudit
        ? structuredClone(proof.rlog_opaque_attribute_audit)
        : null,
      source_status_confounder_route_audit: hasSourceStatusConfounderRouteAudit
        ? structuredClone(proof.source_status_confounder_route_audit)
        : null,
      source_status_local_observable_audit: hasSourceStatusLocalObservableAudit
        ? structuredClone(proof.source_status_local_observable_audit)
        : null,
      target_effect_formula_proof: hasTargetEffectFormulaProof
        ? structuredClone(proof.target_effect_formula_proof)
        : null,
      lucky_packet_component_proof: hasLuckyPacketComponentProof
        ? structuredClone(proof.lucky_packet_component_proof)
        : null,
      mattack_lucky_mitigation_diagnostic: hasMAttackLuckyMitigationDiagnostic
        ? structuredClone(proof.mattack_lucky_mitigation_diagnostic)
        : null,
      attack_lucky_mitigation_diagnostic: hasAttackLuckyMitigationDiagnostic
        ? structuredClone(proof.attack_lucky_mitigation_diagnostic)
        : null,
      lucky_parent_multiplier_proof: hasLuckyParentMultiplierProof
        ? structuredClone(proof.lucky_parent_multiplier_proof)
        : null,
      exact_provider_ownership_proven: hasSameWirePacketOwnership,
      exact_armor_to_damage_equation_proven: false,
      exact_damage_projection_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      blockers: [
        "component-static-scalar-does-not-prove-armor-to-damage-equation",
        "component-static-scalar-does-not-prove-operation-order-or-integer-rounding",
        "component-static-scalar-does-not-prove-counterfactual-projection",
        "component-static-scalar-does-not-prove-canonical-conservation",
        "no-controlled-target-mitigation-counterfactual-pair-observed",
        ...(hasTargetMitigationOfflineExhaustion
          ? ["offline-target-mitigation-search-exhausted-local-control-or-proven-offline-combat-binding-required"]
          : []),
        ...(hasTargetMitigationAcquisitionWorklist
          ? ["target-status-only-relaxation-still-has-no-differing-defense-pair"]
          : []),
        ...(hasTargetMitigationNearPairCandidate
          ? ["transformed-curve-candidate-compatible-but-target-status-confounded"]
          : []),
        ...(hasTargetMitigationStatusConfounderExhaustion
          ? ["all-observed-near-pair-status-confounders-have-zero-exact-target-counterfactual-pairs"]
          : []),
        ...(hasSameAxisStatusEvidence
          ? ["same-axis-target-status-change-has-divergent-damage-outcome"]
          : []),
        ...(hasCounterfactualDiscriminants
          ? ["candidate-counterfactual-rounding-signatures-await-controlled-packet-selection"]
          : []),
        ...(hasSameInputStatusInvariance
          ? ["same-input-status-invariance-observed-common-target-status-confounders-remain"]
          : []),
        ...(hasTargetStatusActionRouteAudit
          ? ["produced-action-routes-prove-no-target-status-modifier-neutrality"]
          : []),
        ...(hasTargetDefensePercentLifecycleProof
          ? ["defense-stat-formula-proven-target-defense-to-damage-projection-unproven"]
          : []),
        ...(hasTargetDefenseStatusDiagnosticRollup
          ? ["expanded-local-status-diagnostics-found-no-independent-control-remote-packets-not-required"]
          : []),
        ...(hasTargetMitigationActorSceneExhaustion
          ? ["actor-scene-exhaustion-found-one-defense-capture-zero-cross-capture-pairs-remote-identity-not-synthesized"]
          : []),
        ...(hasRlogGapWindowAudit
          ? ["gap-bounded-complete-lifecycles-found-but-no-controlled-counterfactual-pair"]
          : []),
        ...(hasRlogTransitionCounterfactualAudit
          ? ["transition-adjacent-search-found-zero-exact-observed-input-controls"]
          : []),
        ...(hasRlogTransitionStagedResidualFrontier
          ? ["closest-transition-pair-equal-output-five-status-differences-incomplete-snapshots"]
          : []),
        ...(hasLuckyPacketComponentProof
          ? ["closest-transition-damage-component-proven-lucky-operator-mitigation-route-open"]
          : []),
        ...(hasMAttackLuckyMitigationDiagnostic
          ? ["mattack-lucky-packets-have-no-observed-target-mitigation-axis-remote-packets-not-required"]
          : []),
        ...(hasAttackLuckyMitigationDiagnostic
          ? ["attack-lucky-packets-have-no-observed-target-mitigation-axis-route-remains-unresolved"]
          : []),
        ...(hasLuckyParentMultiplierProof
          ? [hasExhaustiveLocalSourceAttributeProof
            ? "lucky-parent-relation-complete-224-local-single-attribute-multiplier-candidates-rejected-multi-input-formula-open"
            : hasGroupedLuckyRelationProof
              ? "lucky-parent-relation-complete-three-recorded-multiplier-candidates-rejected-grouped-residual-positive"
              : "lucky-parent-relation-complete-two-obvious-attribute-12530-multiplier-formulas-rejected"]
          : []),
        ...(hasRlogOpaqueAttributeAudit
          ? ["opaque-attributes-443-474-structurally-characterized-semantic-exclusion-unproven"]
          : []),
        ...(hasSourceStatusConfounderRouteAudit
          ? ["source-status-55342-healing-action-route-does-not-prove-modifier-neutrality"]
          : []),
        ...(hasSourceStatusLocalObservableAudit
          ? ["source-status-2207252-local-delta-subset-does-not-prove-general-transfer-or-damage-formula"]
          : []),
        ...(hasTargetEffectFormulaProof
          ? ["gap-bounded-effect-2110092-rows-lack-target-physical-defense-input"]
          : []),
      ],
    });
  }
  return index;
}

function buildCounterfactualRollupResults(
  rollup,
  build,
  providerOwnership,
  integerTransformConstraintIndex = new Map(),
  componentScalarProofIndex = new Map(),
) {
  if (!rollup) return [];
  if (rollup.schema_version !== 1 ||
    rollup.generated_by !== "reviewed-status-effect-counterfactual-rollup") {
    throw new Error("Unsupported status-effect counterfactual rollup schema or generator");
  }
  requireBuild(rollup.game_build, build, "status-effect counterfactual rollup");
  if (rollup.policy?.runtime_authority !== false ||
    rollup.policy?.formula_authority !== false ||
    rollup.policy?.provider_rdps_credit_allowed !== false ||
    rollup.policy?.cross_session_pairing_allowed !== false ||
    rollup.policy?.unresolved_evidence_is_preserved !== true) {
    throw new Error("Status-effect counterfactual rollup authority policy is unsafe");
  }
  const effectId = Number(rollup.effect_id);
  if (!Number.isSafeInteger(effectId) || effectId <= 0) {
    throw new Error("Status-effect counterfactual rollup has an invalid exact effect ID");
  }
  const reviewedProviderOwnership = providerOwnership
    ? validateProviderOwnershipCohort(rollup, providerOwnership)
    : null;
  const results = (rollup.loci ?? []).map((entry) => {
    const locus = String(entry.locus ?? "");
    if (!["source", "target"].includes(locus)) {
      throw new Error(`Status-effect counterfactual rollup ${effectId} has an invalid locus`);
    }
    const exact = structuredClone(entry.exact ?? {});
    const controlledGroups = counterfactualCount(
      exact.controlled_groups,
      `${locus}:${effectId} controlled_groups`,
    );
    const divergentGroups = counterfactualCount(
      exact.divergent_output_groups,
      `${locus}:${effectId} divergent_output_groups`,
    );
    if (divergentGroups > controlledGroups) {
      throw new Error(`Status-effect counterfactual rollup ${locus}:${effectId} has more divergent than controlled groups`);
    }
    const integerTransformConstraints = integerTransformConstraintIndex.get(`${locus}:${effectId}`) ?? null;
    const componentStaticScalar = locus === "target"
      ? componentScalarProofIndex.get(effectId) ?? null
      : null;
    if (integerTransformConstraints) {
      const rollupRlogs = new Set((rollup.runs ?? []).map((run) =>
        path.basename(String(run.rlog?.path ?? ""))
      ).filter(Boolean));
      if (integerTransformConstraints.observed_rlogs.length === 0 ||
        integerTransformConstraints.observed_rlogs.some((rlog) => !rollupRlogs.has(rlog))) {
        throw new Error(`Status-effect integer transform constraints ${locus}:${effectId} do not join the reviewed rollup RLOG cohort`);
      }
      const formulaInputCoverageRlogs = new Set(
        (integerTransformConstraints.provider_formula_input_coverage?.capture_inputs ?? [])
          .map((input) => String(input.rlog ?? ""))
          .filter(Boolean),
      );
      if (formulaInputCoverageRlogs.size > 0 &&
        (formulaInputCoverageRlogs.size !== rollupRlogs.size ||
          [...formulaInputCoverageRlogs].some((rlog) => !rollupRlogs.has(rlog)))) {
        throw new Error(`Status-effect provider formula-input coverage ${locus}:${effectId} does not exactly match the reviewed rollup RLOG cohort`);
      }
      const spHealOperatorRlogs = new Set(
        (integerTransformConstraints.spheal_operator_evidence?.input_rlogs ?? [])
          .map((input) => path.basename(String(input.path ?? "")))
          .filter(Boolean),
      );
      if (spHealOperatorRlogs.size > 0 &&
        (spHealOperatorRlogs.size !== rollupRlogs.size ||
          [...spHealOperatorRlogs].some((rlog) => !rollupRlogs.has(rlog)))) {
        throw new Error(`Status-effect SpHeal operator evidence ${locus}:${effectId} does not exactly match the reviewed rollup RLOG cohort`);
      }
    }
    return {
      effect_id: String(effectId),
      locus,
      status: divergentGroups > 0
        ? "controlled-delta-observed-proof-open"
        : controlledGroups > 0
          ? "controlled-equal-output-observed-proof-open"
          : "no-controlled-counterfactual-pair",
      formula_authority: false,
      runtime_authority: false,
      blockers: uniqueSorted([
        "counterfactual-frontier-declares-no-formula-authority",
        "counterfactual-frontier-declares-no-runtime-authority",
        ...(rollup.blockers ?? []).map(String).filter((blocker) =>
          !reviewedProviderOwnership || blocker !== "provider entity ownership to a player is unproven"
        ),
        ...(reviewedProviderOwnership &&
          !reviewedProviderOwnership.stable_player_character_id_proven_for_every_status_event
          ? ["stable-player-character-id-unproven-for-cross-run-join"]
          : []),
        ...(integerTransformConstraints?.provider_formula_context_summary
          ?.provider_formula_base_input_proven === false
          ? [
              "provider-at-event formula base input is unproven",
              ...(integerTransformConstraints.provider_formula_context_summary
                .missing_or_unproven_inputs ?? []).map((blocker) =>
                `provider formula context: ${String(blocker)}`
              ),
            ]
          : []),
        ...(componentStaticScalar ? componentStaticScalar.blockers : []),
      ]),
      provider_ownership_evidence: reviewedProviderOwnership
        ? structuredClone(reviewedProviderOwnership)
        : null,
      integer_transform_constraints: integerTransformConstraints
        ? structuredClone(integerTransformConstraints)
        : null,
      component_static_scalar_evidence: componentStaticScalar
        ? structuredClone(componentStaticScalar)
        : null,
      observation: { observed_samples: Number(entry.observed_samples ?? 0) },
      exact_recorded_inputs: exact,
      target_current_hp_excluded_diagnostic: structuredClone(
        entry.target_current_hp_excluded_diagnostic ?? {},
      ),
      variants: [],
      rollup_provenance: {
        status: String(rollup.status ?? "unknown"),
        matching_capture_runs: Number(rollup.summary?.matching_capture_runs ?? 0),
        formula_damage_samples: Number(rollup.summary?.formula_damage_samples ?? 0),
      },
    };
  });
  const exactControlledGroups = results.reduce(
    (sum, entry) => sum + Number(entry.exact_recorded_inputs.controlled_groups ?? 0),
    0,
  );
  const exactDivergentGroups = results.reduce(
    (sum, entry) => sum + Number(entry.exact_recorded_inputs.divergent_output_groups ?? 0),
    0,
  );
  if (exactControlledGroups !== Number(rollup.summary?.exact_controlled_groups ?? 0) ||
    exactDivergentGroups !== Number(rollup.summary?.exact_divergent_output_groups ?? 0)) {
    throw new Error("Status-effect counterfactual rollup summary does not match its effect loci");
  }
  return results;
}

function validateProviderOwnershipCohort(rollup, providerOwnership) {
  const runs = rollup.runs ?? [];
  if (!Array.isArray(runs) || runs.length === 0) {
    throw new Error(`Provider ownership proof for effect ${rollup.effect_id} cannot be joined to a rollup without exact run inputs`);
  }
  const rollupInputs = new Map();
  for (const run of runs) {
    const label = path.basename(String(run.rlog?.path ?? "")).toLowerCase();
    const bytes = Number(run.rlog?.bytes);
    const sha256 = String(run.rlog?.sha256 ?? "");
    if (!label || !Number.isSafeInteger(bytes) || bytes <= 0 ||
      !/^sha256:[0-9a-f]{64}$/.test(sha256) || rollupInputs.has(label)) {
      throw new Error(`Counterfactual rollup effect ${rollup.effect_id} has incomplete run provenance`);
    }
    rollupInputs.set(label, { bytes, sha256 });
  }
  if (rollupInputs.size !== providerOwnership.input_rlogs.size) {
    throw new Error(`Provider ownership proof effect ${rollup.effect_id} does not cover the exact counterfactual run cohort`);
  }
  for (const [label, input] of rollupInputs) {
    const ownershipInput = providerOwnership.input_rlogs.get(label);
    if (!ownershipInput || ownershipInput.bytes !== input.bytes || ownershipInput.sha256 !== input.sha256) {
      throw new Error(`Provider ownership proof effect ${rollup.effect_id} input ${label} does not match the counterfactual run cohort`);
    }
  }
  const { input_rlogs: _inputRlogs, ...evidence } = providerOwnership;
  return evidence;
}

function uniqueCounterfactualResultLoci(results) {
  const seen = new Set();
  for (const entry of results) {
    const key = `${String(entry.locus)}:${String(entry.effect_id)}`;
    if (seen.has(key)) throw new Error(`Duplicate counterfactual result locus ${key}`);
    seen.add(key);
  }
  return results.sort((left, right) =>
    compareIdentifiers(left.effect_id, right.effect_id) || compareText(left.locus, right.locus)
  );
}

function buildRuntimeEffectResults(aggregate, runtimeAttributionEvidence, componentRoutingProof, build) {
  const terminalEffects = aggregate.dreamscope_terminal_effects ?? [];
  const readinessEffects = aggregate.remote_rdps_readiness?.effects ?? [];
  const readinessByEffect = uniqueIndex(readinessEffects, "effect_id", "runtime effect readiness");
  const componentRoutesByEffect = uniqueIndex(
    componentRoutingProof.effect_routes ?? [],
    "effect_id",
    "runtime effect component route",
  );

  const results = terminalEffects.map((effect) => {
    const effectId = String(effect.effect_id);
    const readiness = readinessByEffect.get(effectId);
    const componentRoute = componentRoutesByEffect.get(effectId) ?? null;
    const states = effect.status_states ?? {};
    const applied = Number(states.applied ?? 0) + Number(states.refreshed ?? 0) + Number(states.stacked ?? 0);
    const terminal = Number(states.removed ?? 0) + Number(states.consumed ?? 0);
    const providerPairs = effect.provider_recipient_observations ?? [];
    const externalPairs = providerPairs.filter(isExternalObservation);
    const sourceObservations = effect.source_observations ?? [];
    const routeExact = sourceObservations.length > 0 && sourceObservations.every((entry) => entry.route_resolution === "exact");
    const providerRecipientExact = providerPairs.length > 0 && providerPairs.every((entry) => entry.provider_actor_id !== null && entry.provider_actor_id !== undefined);
    const lifecycleExact = applied > 0 && terminal > 0 && Number(effect.ambiguous_status_removals ?? 0) === 0 &&
      Number(effect.ambiguous_provider_window_damage_events ?? 0) === 0;
    const provenNonOutgoing = componentRoute?.proven_no_outgoing_attribution === true;
    const coarseExternalCandidate = Boolean(readiness?.external_attribution_candidate) || externalPairs.length > 0;
    const externalCandidate = provenNonOutgoing
      ? false
      : Boolean(componentRoute?.runtime_credit_candidate) || coarseExternalCandidate;
    const externalComponents = (componentRoute?.components ?? []).filter((component) =>
      component.transfer_eligibility === "external-recipient-candidate"
    );
    const declaredComponentScalars = externalComponents.flatMap((component) =>
      (component.values ?? []).map((value) => ({
        source_rule_id: component.source_rule_id ?? null,
        component_key: component.component_key ?? null,
        label: component.label ?? null,
        value_resolution: component.value_resolution ?? null,
        raw_text: value.raw_text ?? null,
        unit: value.unit ?? null,
        value: value.value ?? null,
        decimal_value: value.decimal_value ?? null,
        formula_amount: value.formula_amount === true,
        formula_replay_status: component.formula_replay_status ?? null,
      }))
    );
    const declaredComponentScalarResolved = externalComponents.length > 0 && externalComponents.every((component) =>
      component.value_resolution === "single" && component.values?.length === 1 &&
      component.values[0]?.formula_amount === true &&
      isFiniteNumber(component.values[0]?.decimal_value)
    );
    const formulaPlacementStatuses = uniqueSorted(
      externalComponents.map((component) => component.formula_replay_status ?? "unresolved"),
    );
    const formulaPlacementResolved = externalComponents.length > 0 && externalComponents.every((component) =>
      isExactFormulaPlacementStatus(component.formula_replay_status)
    );
    const scalarResolution = readiness?.scalar_resolution ?? effect.remote_calculation?.scalar_resolution ?? "unresolved";
    const scalarResolved = scalarResolution !== "unresolved";
    const retainedExternalDamageEvents = Number(
      readiness?.retained_external_provider_window_damage_events ?? effect.external_provider_window_damage_events ?? 0,
    );
    const retainedExternalDamage = String(
      readiness?.retained_external_provider_window_damage ?? effect.external_provider_window_damage ?? "0",
    );
    const blockers = [];
    if (!provenNonOutgoing) {
      if (!readiness) blockers.push("runtime-readiness-record-missing");
      if (externalCandidate && !routeExact) blockers.push("exact-packet-or-formula-route-missing");
      if (externalCandidate && !providerRecipientExact) blockers.push("exact-provider-recipient-missing");
      if (externalCandidate && externalPairs.length === 0) blockers.push("external-provider-scope-unproven");
      if (externalCandidate && !lifecycleExact) blockers.push("apply-and-terminal-lifecycle-incomplete-or-ambiguous");
      if (externalCandidate && !scalarResolved) {
        if (!declaredComponentScalarResolved) blockers.push("declared-component-scalar-unresolved");
        else if (!formulaPlacementResolved) blockers.push("exact-counterfactual-formula-placement-unresolved");
        else blockers.push("runtime-applied-magnitude-unresolved");
      }
      if (externalCandidate && retainedExternalDamageEvents === 0) blockers.push("external-recipient-window-damage-missing");
    }

    const runtimeModelReady = externalCandidate && blockers.length === 0 && Boolean(readiness?.calculation_ready);
    // Recipient-window damage proves occurrence and bounds the affected packet
    // rows. It does not by itself prove the counterfactual delta. Keep the
    // effect open until projected contribution reconciles against those rows.
    if (runtimeModelReady) blockers.push("strict-counterfactual-projection-and-conservation-unproven");

    const status = provenNonOutgoing
      ? "packet-observed-non-outgoing-context"
      : !externalCandidate
      ? "packet-observed-non-external"
      : runtimeModelReady
        ? "runtime-model-ready-awaiting-strict-conservation"
        : "runtime-external-open";
    return {
      effect_id: effectId,
      source_match: effect.source_match ?? null,
      status,
      blockers: uniqueSorted(blockers),
      gates: {
        exact_route: routeExact,
        provider_recipient: providerRecipientExact,
        external_provider_scope: externalPairs.length > 0,
        apply_and_terminal_lifecycle: lifecycleExact,
        runtime_scalar: scalarResolved,
        retained_external_window_damage: retainedExternalDamageEvents > 0,
        strict_counterfactual_conservation: false,
      },
      evidence: {
        status_states: states,
        provider_recipient_observations: providerPairs.length,
        external_provider_recipient_observations: externalPairs.length,
        ambiguous_status_removals: Number(effect.ambiguous_status_removals ?? 0),
        ambiguous_provider_window_damage_events: Number(effect.ambiguous_provider_window_damage_events ?? 0),
        source_observations: sourceObservations.length,
        observed_provider_scope: readiness?.observed_provider_scope ?? effect.remote_calculation?.observed_provider_scope ?? "unknown",
        scalar_resolution: scalarResolution,
        declared_component_scalar_resolved: declaredComponentScalarResolved,
        declared_component_scalars: declaredComponentScalars,
        formula_placement_resolved: formulaPlacementResolved,
        formula_placement_statuses: formulaPlacementStatuses,
        runtime_calculation_ready: Boolean(readiness?.calculation_ready),
        retained_external_provider_window_damage_events: retainedExternalDamageEvents,
        retained_external_provider_window_damage: retainedExternalDamage,
        component_routing: componentRoute
          ? {
              route_class: componentRoute.route_class,
              runtime_credit_candidate: componentRoute.runtime_credit_candidate,
              proven_no_outgoing_attribution: componentRoute.proven_no_outgoing_attribution,
              component_counts: componentRoute.component_counts,
              source_rule_ids: componentRoute.source_rule_ids,
            }
          : null,
      },
    };
  });

  promoteExactRuntimeAttribution(results, runtimeAttributionEvidence, build);
  results.sort((left, right) => compareIdentifiers(left.effect_id, right.effect_id));

  const resultIds = new Set(results.map((entry) => entry.effect_id));
  const readinessOnly = [...readinessByEffect.keys()].filter((effectId) => !resultIds.has(String(effectId)));
  if (readinessOnly.length > 0) {
    throw new Error(`Runtime readiness records have no preserved terminal effect: ${readinessOnly.sort(compareIdentifiers).join(", ")}`);
  }
  return results;
}

function isFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value);
}

function isExactFormulaPlacementStatus(status) {
  return status === "exact-current-build-formula-placement-proven" ||
    status === "packet-replay-proven-exact-target";
}

function greatestCommonDivisor(left, right) {
  let a = left < 0n ? -left : left;
  let b = right < 0n ? -right : right;
  while (b !== 0n) {
    const remainder = a % b;
    a = b;
    b = remainder;
  }
  return a === 0n ? 1n : a;
}

function normalizeRational(numerator, denominator) {
  if (denominator === 0n) throw new Error("Exact attribution rational denominator cannot be zero");
  const sign = denominator < 0n ? -1n : 1n;
  const signedNumerator = numerator * sign;
  const positiveDenominator = denominator * sign;
  const divisor = greatestCommonDivisor(signedNumerator, positiveDenominator);
  return { numerator: signedNumerator / divisor, denominator: positiveDenominator / divisor };
}

function addRational(left, right) {
  return normalizeRational(
    left.numerator * right.denominator + right.numerator * left.denominator,
    left.denominator * right.denominator,
  );
}

function exactDelta(entry) {
  let total = normalizeRational(BigInt(String(entry.exact_integer_delta ?? "0")), 1n);
  for (const rational of entry.exact_rational_deltas ?? []) {
    const count = BigInt(String(rational.contribution_count ?? 1));
    total = addRational(total, normalizeRational(
      BigInt(String(rational.numerator ?? "0")) * count,
      BigInt(String(rational.denominator ?? "0")),
    ));
  }
  return total;
}

function reportEffectDelta(report, effectId) {
  const rationalEntries = (report.summary?.rational_effects ?? [])
    .filter((entry) => String(entry.effect_id) === effectId);
  if (rationalEntries.length > 0) {
    return rationalEntries.reduce(
      (sum, entry) => addRational(sum, normalizeRational(
        BigInt(String(entry.numerator ?? "0")),
        BigInt(String(entry.denominator ?? "0")),
      )),
      normalizeRational(0n, 1n),
    );
  }
  return normalizeRational(
    (report.summary?.effects ?? [])
      .filter((entry) => String(entry.effect_id) === effectId)
      .reduce((sum, entry) => sum + BigInt(String(entry.amount ?? "0")), 0n),
    1n,
  );
}

function rationalEquals(left, right) {
  return left.numerator === right.numerator && left.denominator === right.denominator;
}

function positiveRational(value) {
  return value.numerator > 0n && value.denominator > 0n;
}

function serializedRational(value) {
  return { numerator: value.numerator.toString(), denominator: value.denominator.toString() };
}

function promoteExactRuntimeAttribution(results, evidence, build) {
  if (!evidence) return;
  requireBuild(evidence.runtime_rule_build, build, "runtime attribution evidence");
  const catalogs = (evidence.relationship_catalog ?? []).filter((entry) =>
    String(entry.client_build) === String(build) &&
    entry.deployment_id === "global" &&
    entry.proof_status === "packet_replay_proven_exact_target" &&
    entry.damage_context_complete === true &&
    Number(entry.damage_event_count ?? 0) > 0 &&
    positiveRational(exactDelta(entry))
  );
  const catalogsByEffect = groupBy(catalogs, (entry) => String(entry.effect_id));

  const byEffect = new Map(results.map((entry) => [String(entry.effect_id), entry]));
  for (const [effectId, effectCatalogs] of catalogsByEffect) {
    const reports = (evidence.reports ?? []).filter((report) =>
      String(report.client_build) === String(build) &&
      report.deployment_id === "global" &&
      report.runtime_target_match === true &&
      report.conserved === true &&
      Number(report.emitted_contribution_events_by_effect?.[effectId] ?? 0) > 0
    );
    const emittedEvents = reports.reduce(
      (sum, report) => sum + Number(report.emitted_contribution_events_by_effect?.[effectId] ?? 0),
      0,
    );
    const missingSourceStatuses = reports.reduce(
      (sum, report) => sum + Number(report.summary?.missing_source_status_count ?? 0),
      0,
    );
    const relationships = reports.flatMap((report) =>
      (report.influence_relationships ?? []).filter((entry) => String(entry.effect_id) === effectId),
    );
    const relationshipsExact = relationships.length > 0 && relationships.every((entry) =>
      entry.damage_context_complete === true &&
      String(entry.provider_actor_id) !== String(entry.recipient_actor_id) &&
      entry.provider_entity_uuid && entry.recipient_entity_uuid &&
      entry.damage_source_actor_id && entry.damage_source_entity_uuid &&
      entry.target_actor_id && entry.target_entity_uuid &&
      Number(entry.damage_event_count ?? 0) > 0 &&
      positiveRational(exactDelta(entry))
    );
    const catalogEvents = effectCatalogs.reduce((sum, entry) => sum + Number(entry.damage_event_count ?? 0), 0);
    const catalogDelta = effectCatalogs.reduce(
      (sum, entry) => addRational(sum, exactDelta(entry)),
      normalizeRational(0n, 1n),
    );
    const relationshipEvents = relationships.reduce((sum, entry) => sum + Number(entry.damage_event_count ?? 0), 0);
    const relationshipDelta = relationships.reduce(
      (sum, entry) => addRational(sum, exactDelta(entry)),
      normalizeRational(0n, 1n),
    );
    const reportSummariesExact = reports.every((report) => {
      const reportRelationships = (report.influence_relationships ?? [])
        .filter((entry) => String(entry.effect_id) === effectId);
      const reportRelationshipEvents = reportRelationships
        .reduce((sum, entry) => sum + Number(entry.damage_event_count ?? 0), 0);
      const reportRelationshipDelta = reportRelationships.reduce(
        (sum, entry) => addRational(sum, exactDelta(entry)),
        normalizeRational(0n, 1n),
      );
      return reportRelationshipEvents === Number(report.emitted_contribution_events_by_effect?.[effectId] ?? 0) &&
        rationalEquals(reportRelationshipDelta, reportEffectDelta(report, effectId));
    });
    const protocolPackDigests = uniqueSorted(effectCatalogs.map((entry) => entry.protocol_pack_digest));
    if (reports.length === 0 || emittedEvents !== catalogEvents || relationshipEvents !== catalogEvents ||
      !rationalEquals(relationshipDelta, catalogDelta) || missingSourceStatuses !== 0 ||
      !relationshipsExact || !reportSummariesExact || protocolPackDigests.length !== 1) {
      continue;
    }

    const ambiguousProviderWindows = reports.reduce(
      (sum, report) => sum + Number(report.target_vulnerability_audit_gates?.multiple_external_active_providers ?? 0),
      0,
    );
    const promoted = {
      ...(byEffect.get(effectId) ?? { effect_id: effectId, source_match: null }),
      status: "runtime-attribution-promoted-exact-subset",
      resolution_scope: "current-build-conserved-unambiguous-event-subset",
      full_effect_family_resolved: false,
      blockers: [],
      gates: {
        exact_route: true,
        provider_recipient: true,
        external_provider_scope: true,
        apply_and_terminal_lifecycle: true,
        runtime_scalar: true,
        retained_external_window_damage: true,
        strict_counterfactual_conservation: true,
      },
      evidence: {
        ...(byEffect.get(effectId)?.evidence ?? {}),
        runtime_calculation_ready: true,
        exact_replay_sessions: reports.length,
        exact_replay_source_paths: uniqueSorted(reports.map((report) => report.source_path)),
        exact_replay_damage_events: emittedEvents,
        ...(catalogDelta.denominator === 1n
          ? { exact_replay_attributed_bonus_damage: catalogDelta.numerator.toString() }
          : {}),
        exact_replay_attributed_bonus_damage_rational: serializedRational(catalogDelta),
        exact_replay_relationships: relationships.length,
        exact_replay_affected_damage_ids: uniqueSorted(
          effectCatalogs.map((entry) => entry.affected_damage_id),
          compareIdentifiers,
        ),
        ...(effectCatalogs.length === 1
          ? { exact_replay_affected_damage_id: String(effectCatalogs[0].affected_damage_id) }
          : {}),
        exact_replay_protocol_pack_digest: protocolPackDigests[0],
        ambiguous_provider_windows_remain_deferred: true,
        ambiguous_provider_window_events: ambiguousProviderWindows,
      },
    };
    if (byEffect.has(effectId)) {
      const index = results.findIndex((entry) => String(entry.effect_id) === effectId);
      results[index] = promoted;
    } else {
      results.push(promoted);
    }
    byEffect.set(effectId, promoted);
  }
}

function evaluateObligation(context) {
  const { manifestObligation, observed, aggregate, sourceRuleIds } = context;
  const effectIds = uniqueSorted(manifestObligation.selectors?.effect_ids ?? [], compareIdentifiers);
  const componentScalarProof = effectIds.length === 1
    ? context.componentScalarProofIndex?.get(Number(effectIds[0])) ?? null
    : null;
  const componentScalarProviderOwnershipComplete = componentScalarProof
    ? componentScalarProof.exact_provider_ownership_proven === true
    : true;
  const transferGate = normalizeTransferGate(manifestObligation.evidence);
  const transferClass = classifyTransferGate(transferGate.kind);
  const transferEligible = transferClass === "externally-transferable";
  const requiredKinds = new Set(manifestObligation.required_event_kinds ?? []);
  const hasCandidate = observed.coverage_state !== "no-candidate-evidence" &&
    (Number(observed.direct_matches ?? 0) + Number(observed.contextual_matches ?? 0) > 0);
  const rawExternalObservations = (observed.provider_recipient_observations ?? []).filter(isExternalObservation);
  const rawProjectedExternalObservations = (observed.projected_provider_recipient_observations ?? []).filter(isExternalObservation);
  const externalObservations = transferEligible ? rawExternalObservations : [];
  const projectedExternalObservations = transferEligible ? rawProjectedExternalObservations : [];
  const states = observed.status_states ?? {};
  const applied = Number(states.applied ?? 0);
  const terminal = Number(states.removed ?? 0) + Number(states.consumed ?? 0);
  const lifecycleComplete = applied > 0 && terminal > 0 && Number(observed.ambiguous_status_removals ?? 0) === 0 &&
    Number(observed.ambiguous_provider_window_damage_events ?? 0) === 0;
  const formulaRequired = requiredKinds.has("formula_inputs") || [
    "external-target-state-counterfactual",
    "external-recipient-counterfactual",
  ].includes(transferGate.kind);
  const snapshots = observed.formula_input_snapshots ?? [];
  const snapshotCounts = formulaSnapshotCounts(observed);
  const formulaComplete = !formulaRequired || (snapshotCounts.total > 0 && snapshotCounts.complete === snapshotCounts.total);
  const validProjectionEvents = Number(observed.projected_integer_events ?? 0) + Number(observed.projected_rational_events ?? 0);
  const projectionComplete = validProjectionEvents > 0 && Number(observed.projected_invalid_events ?? 0) === 0 &&
    Number(observed.projected_excluded_events ?? 0) === 0 && projectedExternalObservations.length > 0;
  const packetRows = observed.packet_damage_rows ?? [];
  const packetRowCount = projectedCount(observed.packet_damage_row_count, packetRows.length);
  const packetConservationComplete = packetRowCount > 0 && projectionComplete;
  const sourceConservationComplete = packetRowCount > 0 &&
    String(observed.source_owned_conservation_status ?? "") === "conserved";
  const staticStates = sourceRuleIds.map((sourceRuleId) => staticStateForSource(sourceRuleId, formulaRequired, context.staticBySource));
  const staticComplete = staticStates.every((entry) => entry.state === "resolved" || entry.state === "not-required");
  const buildMatches = String(aggregate.manifest_game_build) === String(context.build) &&
    !aggregate.provisional_build_mismatch &&
    (aggregate.observed_game_builds ?? []).every((build) => String(build) === String(context.build));

  const blockers = [];
  if (!buildMatches) blockers.push("matching-build-packet-evidence-missing");
  if (!hasCandidate) blockers.push("candidate-packet-evidence-missing");
  if (!staticComplete) blockers.push("static-formula-model-unresolved");
  if (hasCandidate && transferEligible && (observed.provider_recipient_observations ?? []).length === 0) blockers.push("provider-recipient-evidence-missing");
  else if (hasCandidate && transferEligible && externalObservations.length === 0) blockers.push("external-provider-scope-unproven");
  if (hasCandidate && transferEligible && externalObservations.length > 0 &&
    componentScalarProof && !componentScalarProviderOwnershipComplete) {
    blockers.push("component-static-scalar-provider-ownership-unproven");
  }
  if (hasCandidate && transferEligible && externalObservations.length > 0 && !lifecycleComplete) blockers.push("lifecycle-incomplete-or-ambiguous");
  if (hasCandidate && transferEligible && externalObservations.length > 0 && !formulaComplete) blockers.push("required-formula-input-snapshots-incomplete");
  if (hasCandidate && transferEligible && externalObservations.length > 0 && formulaComplete && !projectionComplete) blockers.push("counterfactual-projection-unproven");
  if (hasCandidate && transferEligible && externalObservations.length > 0 && formulaComplete && projectionComplete && !packetConservationComplete) blockers.push("packet-damage-conservation-unproven");
  if (hasCandidate && transferClass === "nontransfer" && !sourceConservationComplete) blockers.push("source-owned-output-conservation-unproven");
  if (hasCandidate && transferClass === "component-scoped") blockers.push("component-transfer-gates-not-materialized");
  if (hasCandidate && transferClass === "unresolved") blockers.push("provider-recipient-transfer-gate-unresolved");
  if (hasCandidate && transferClass === "non-outgoing") blockers.push("non-outgoing-context-not-an-rdps-transfer");
  if (hasCandidate && transferClass === "missing") blockers.push("typed-transfer-gate-missing");

  const status = classifyStatus({
    buildMatches,
    hasCandidate,
    transferClass,
    transferEligible,
    sourceConservationComplete,
    staticComplete,
    providerObservations: (observed.provider_recipient_observations ?? []).length,
    externalObservations: externalObservations.length,
    componentScalarProofPresent: componentScalarProof !== null,
    componentScalarProviderOwnershipComplete,
    lifecycleComplete,
    formulaComplete,
    projectionComplete,
    packetConservationComplete,
  });
  const historicalLeads = uniqueSorted(effectIds.flatMap((effectId) => context.historicalProofsByEffect.get(String(effectId)) ?? []));

  return {
    obligation_id: manifestObligation.obligation_id,
    domain: manifestObligation.domain,
    subject_kind: manifestObligation.subject_kind,
    subject_id: manifestObligation.subject_id,
    subject_name: manifestObligation.subject_name,
    source_rule_ids: sourceRuleIds,
    shared_model_keys: uniqueSorted(sourceRuleIds.flatMap((sourceRuleId) => context.workbenchModelsBySource.get(sourceRuleId) ?? [])),
    effect_ids: effectIds,
    transfer_gate: transferGate,
    transfer_class: transferClass,
    status,
    blockers,
    gates: {
      matching_build: buildMatches,
      candidate_evidence: hasCandidate,
      static_formula_model: staticComplete,
      transfer_eligibility: transferEligible,
      external_provider_scope: externalObservations.length > 0,
      ...(componentScalarProof
        ? { exact_component_scalar_provider_ownership: componentScalarProviderOwnershipComplete }
        : {}),
      lifecycle: lifecycleComplete,
      formula_inputs: formulaComplete,
      counterfactual_projection: projectionComplete,
      packet_conservation: transferEligible ? packetConservationComplete : sourceConservationComplete,
    },
    evidence: {
      coverage_state: observed.coverage_state,
      observed_event_kinds: observed.observed_event_kinds ?? [],
      missing_event_kinds: observed.missing_event_kinds ?? [],
      provider_recipient_observations: (observed.provider_recipient_observations ?? []).length,
      raw_external_provider_recipient_observations: rawExternalObservations.length,
      eligible_external_provider_recipient_observations: externalObservations.length,
      status_states: states,
      ambiguous_status_removals: Number(observed.ambiguous_status_removals ?? 0),
      ambiguous_provider_window_damage_events: Number(observed.ambiguous_provider_window_damage_events ?? 0),
      recipient_window_damage_events: Number(observed.recipient_window_damage_events ?? 0),
      target_window_damage_events: Number(observed.target_window_damage_events ?? 0),
      single_provider_window_damage_events: Number(observed.single_provider_window_damage_events ?? 0),
      formula_input_snapshots: snapshotCounts.total,
      complete_formula_input_snapshots: snapshotCounts.complete,
      packet_damage_rows: packetRowCount,
      source_owned_conservation_status: observed.source_owned_conservation_status ?? null,
      projection_statuses: observed.projection_statuses ?? [],
      projected_external_provider_recipient_observations: projectedExternalObservations.length,
      projected_integer_events: Number(observed.projected_integer_events ?? 0),
      projected_rational_events: Number(observed.projected_rational_events ?? 0),
      projected_invalid_events: Number(observed.projected_invalid_events ?? 0),
      projected_excluded_events: Number(observed.projected_excluded_events ?? 0),
      exact_terminal_effect_enrichment: observed.exact_terminal_effect_enrichment ?? null,
      component_static_scalar_provider_ownership: componentScalarProof
        ? {
            proof: structuredClone(componentScalarProof.proof),
            exact_provider_ownership_proven: componentScalarProviderOwnershipComplete,
            unresolved_provider_status_events:
              Number(componentScalarProof.unresolved_provider_status_events),
            same_wire_packet_player_owned_status_events:
              Number(componentScalarProof.same_wire_packet_player_owned_status_events ?? 0),
            prior_status_instance_player_owned_status_events:
              Number(componentScalarProof.prior_status_instance_player_owned_status_events ?? 0),
            gap_groups: componentScalarProof.provider_ownership_gap_worklist
              ? Number(componentScalarProof.provider_ownership_gap_worklist.gap_groups)
              : null,
            unresolved_events_with_same_source_separate_stable_player_resolution:
              componentScalarProof.provider_ownership_gap_worklist
                ? Number(componentScalarProof.provider_ownership_gap_worklist
                  .unresolved_events_with_same_source_separate_stable_player_resolution)
                : null,
            unresolved_events_without_same_source_stable_player_resolution:
              componentScalarProof.provider_ownership_gap_worklist
                ? Number(componentScalarProof.provider_ownership_gap_worklist
                  .unresolved_events_without_same_source_stable_player_resolution)
                : null,
          }
        : null,
    },
    static_sources: staticStates,
    historical_proof_leads: historicalLeads,
  };
}

function buildAggregateObservationIndex(obligations) {
  const byId = uniqueIndex(obligations, "obligation_id", "aggregate obligation");
  const byRuntimeSelectorContract = new Map();
  for (const observed of obligations) {
    const selectors = parseAggregateSelectorContract(observed);
    if (!selectors) continue;
    const key = runtimeSelectorCorrelationKey(observed, selectors);
    if (!byRuntimeSelectorContract.has(key)) byRuntimeSelectorContract.set(key, []);
    byRuntimeSelectorContract.get(key).push(observed);
  }
  return { byId, byRuntimeSelectorContract };
}

function buildTerminalEffectObservationIndex(effects) {
  const result = new Map();
  for (const effect of effects) {
    const effectId = effect?.effect_id;
    if (effectId === undefined || effectId === null || effectId === "") {
      throw new Error("Terminal effect observation is missing effect_id");
    }
    const key = String(effectId);
    if (result.has(key)) throw new Error(`Duplicate terminal effect observation ${key}`);
    result.set(key, effect);
  }
  return result;
}

function isExactTerminalEffectObservation(effect) {
  if (String(effect?.source_match?.resolution ?? "") !== "exact") return false;
  return (effect.source_observations ?? []).every(
    (source) => String(source?.route_resolution ?? "") === "exact",
  );
}

function enrichObservationWithExactTerminalEffects(observed, obligation, terminalEffectIndex) {
  const selectedEffectIds = uniqueSorted(
    obligation.selectors?.effect_ids ?? [],
    compareIdentifiers,
  );
  const exactEffects = selectedEffectIds
    .map((effectId) => terminalEffectIndex.get(String(effectId)))
    .filter((effect) => effect && isExactTerminalEffectObservation(effect));
  if (exactEffects.length === 0) return observed;

  const observedKinds = new Set(observed.observed_event_kinds ?? []);
  const lifecycleEventCount = exactEffects.reduce(
    (total, effect) => total + Object.values(effect.status_states ?? {})
      .reduce((subtotal, value) => subtotal + Number(value ?? 0), 0),
    0,
  );
  const damageEventCount = exactEffects.reduce(
    (total, effect) => total + Math.max(
      Number(effect.recipient_window_damage_events ?? 0),
      Number(effect.target_window_damage_events ?? 0),
      Number(effect.external_provider_window_damage_events ?? 0),
      Number(effect.single_provider_window_damage_events ?? 0),
    ),
    0,
  );
  if (lifecycleEventCount > 0) observedKinds.add("status");
  if (damageEventCount > 0) observedKinds.add("damage");

  const requiredKinds = uniqueSorted(
    obligation.required_event_kinds ?? observed.required_event_kinds ?? [],
  );
  const missingKinds = requiredKinds.filter((kind) => !observedKinds.has(kind));
  const mergedStatusStates = { ...(observed.status_states ?? {}) };
  for (const effect of exactEffects) {
    for (const [state, count] of Object.entries(effect.status_states ?? {})) {
      mergedStatusStates[state] = Math.max(
        Number(mergedStatusStates[state] ?? 0),
        Number(count ?? 0),
      );
    }
  }

  const providerRecipientByKey = new Map();
  for (const entry of observed.provider_recipient_observations ?? []) {
    mergeProviderRecipientObservation(providerRecipientByKey, entry, null);
  }
  for (const effect of exactEffects) {
    for (const entry of effect.provider_recipient_observations ?? []) {
      mergeProviderRecipientObservation(providerRecipientByKey, entry, effect.effect_id);
    }
  }

  const maximumField = (field) => Math.max(
    Number(observed[field] ?? 0),
    ...exactEffects.map((effect) => Number(effect[field] ?? 0)),
  );
  const exactEffectEvidence = exactEffects.map((effect) => ({
    effect_id: String(effect.effect_id),
    source_resolution: effect.source_match.resolution,
    source_observations: (effect.source_observations ?? []).length,
    status_states: effect.status_states ?? {},
    provider_recipient_observations: (effect.provider_recipient_observations ?? []).length,
    recipient_window_damage_events: Number(effect.recipient_window_damage_events ?? 0),
    external_provider_window_damage_events: Number(effect.external_provider_window_damage_events ?? 0),
    target_window_damage_events: Number(effect.target_window_damage_events ?? 0),
    single_provider_window_damage_events: Number(effect.single_provider_window_damage_events ?? 0),
  }));

  return {
    ...observed,
    coverage_state: missingKinds.length === 0
      ? "candidate-event-coverage-complete"
      : "partial-candidate-event-coverage",
    required_event_kinds: requiredKinds,
    observed_event_kinds: uniqueSorted([...observedKinds]),
    missing_event_kinds: missingKinds,
    direct_matches: Math.max(Number(observed.direct_matches ?? 0), lifecycleEventCount),
    provider_recipient_observations: [...providerRecipientByKey.values()].sort(compareProviderRecipientObservations),
    status_states: mergedStatusStates,
    status_instance_ids: uniqueSorted([
      ...(observed.status_instance_ids ?? []),
      ...exactEffects.flatMap((effect) => effect.status_instance_ids ?? []),
    ], compareIdentifiers),
    ambiguous_status_removals: maximumField("ambiguous_status_removals"),
    ambiguous_provider_window_damage_events: maximumField("ambiguous_provider_window_damage_events"),
    recipient_window_damage_events: maximumField("recipient_window_damage_events"),
    external_provider_window_damage_events: maximumField("external_provider_window_damage_events"),
    target_window_damage_events: maximumField("target_window_damage_events"),
    single_provider_window_damage_events: maximumField("single_provider_window_damage_events"),
    exact_terminal_effect_enrichment: {
      policy: "exact-effect-id-and-exact-source-route-only",
      effect_ids: exactEffectEvidence.map((effect) => effect.effect_id),
      exact_effect_count: exactEffectEvidence.length,
      lifecycle_event_count: lifecycleEventCount,
      provider_recipient_observation_count: exactEffectEvidence.reduce(
        (total, effect) => total + effect.provider_recipient_observations,
        0,
      ),
      source_observation_count: exactEffectEvidence.reduce(
        (total, effect) => total + effect.source_observations,
        0,
      ),
      recipient_window_damage_events: exactEffectEvidence.reduce(
        (total, effect) => total + effect.recipient_window_damage_events,
        0,
      ),
      external_provider_window_damage_events: exactEffectEvidence.reduce(
        (total, effect) => total + effect.external_provider_window_damage_events,
        0,
      ),
      target_window_damage_events: exactEffectEvidence.reduce(
        (total, effect) => total + effect.target_window_damage_events,
        0,
      ),
      single_provider_window_damage_events: exactEffectEvidence.reduce(
        (total, effect) => total + effect.single_provider_window_damage_events,
        0,
      ),
      effects: exactEffectEvidence,
    },
  };
}

function mergeProviderRecipientObservation(index, entry, fallbackEffectId) {
  const provider = entry?.provider_actor_id ?? entry?.provider ?? entry?.source_actor_id;
  const recipient = entry?.recipient_actor_id ?? entry?.recipient ?? entry?.target_actor_id;
  const effectId = entry?.effect_id ?? fallbackEffectId;
  const key = [
    effectId === undefined || effectId === null ? "unscoped" : String(effectId),
    provider === undefined || provider === null ? "unknown" : String(provider),
    recipient === undefined || recipient === null ? "unknown" : String(recipient),
  ].join(":");
  const normalized = {
    ...entry,
    ...(effectId === undefined || effectId === null ? {} : { effect_id: String(effectId) }),
    observation_count: Number(entry?.observation_count ?? 1),
  };
  const existing = index.get(key);
  if (!existing) {
    index.set(key, normalized);
    return;
  }
  index.set(key, {
    ...existing,
    ...normalized,
    observation_count: Math.max(
      Number(existing.observation_count ?? 1),
      normalized.observation_count,
    ),
  });
}

function compareProviderRecipientObservations(left, right) {
  return compareIdentifiers(left.effect_id ?? "", right.effect_id ?? "") ||
    compareIdentifiers(left.provider_actor_id ?? left.provider ?? left.source_actor_id ?? "", right.provider_actor_id ?? right.provider ?? right.source_actor_id ?? "") ||
    compareIdentifiers(left.recipient_actor_id ?? left.recipient ?? left.target_actor_id ?? "", right.recipient_actor_id ?? right.recipient ?? right.target_actor_id ?? "");
}

function resolveAggregateObservation(manifestObligation, aggregateIndex, usedAggregateObligationIds) {
  const manifestId = String(manifestObligation.obligation_id);
  const exact = aggregateIndex.byId.get(manifestId);
  if (exact) {
    claimAggregateObservation(manifestId, exact, usedAggregateObligationIds);
    return {
      kind: "exact-id",
      observed: exact,
      runtime_selector_contract_sha256: runtimeSelectorContractHash(manifestObligation.selectors ?? {}),
    };
  }

  const key = runtimeSelectorCorrelationKey(manifestObligation, manifestObligation.selectors ?? {});
  const candidates = aggregateIndex.byRuntimeSelectorContract.get(key) ?? [];
  if (candidates.length === 0) {
    return {
      kind: "manifest-new-no-observation",
      observed: emptyObservationForManifestObligation(manifestObligation),
      runtime_selector_contract_sha256: runtimeSelectorContractHash(manifestObligation.selectors ?? {}),
    };
  }
  if (candidates.length > 1) {
    throw new Error(
      `Aggregate runtime selector contract for ${manifestId} is ambiguous across ` +
      candidates.map((entry) => entry.obligation_id).sort(compareText).join(", "),
    );
  }
  const observed = candidates[0];
  claimAggregateObservation(manifestId, observed, usedAggregateObligationIds);
  return {
    kind: "runtime-selector-rekey",
    observed,
    runtime_selector_contract_sha256: runtimeSelectorContractHash(manifestObligation.selectors ?? {}),
  };
}

function emptyObservationForManifestObligation(obligation) {
  return {
    obligation_id: `unobserved:${obligation.obligation_id}`,
    coverage_state: "no-candidate-evidence",
    selector_contract: stableStringify(obligation.selectors ?? {}),
    required_event_kinds: obligation.required_event_kinds ?? [],
    observed_event_kinds: [],
    missing_event_kinds: obligation.required_event_kinds ?? [],
    direct_matches: 0,
    contextual_matches: 0,
    provider_recipient_observations: [],
    status_states: {},
    ambiguous_status_removals: 0,
    ambiguous_provider_window_damage_events: 0,
    recipient_window_damage_events: 0,
    target_window_damage_events: 0,
    single_provider_window_damage_events: 0,
    formula_input_snapshots: [],
    packet_damage_rows: [],
    projection_statuses: [],
    projected_provider_recipient_observations: [],
    projected_integer_events: 0,
    projected_rational_events: 0,
    projected_invalid_events: 0,
    projected_excluded_events: 0,
  };
}

function claimAggregateObservation(manifestId, observed, usedAggregateObligationIds) {
  const aggregateId = String(observed.obligation_id);
  if (usedAggregateObligationIds.has(aggregateId)) {
    throw new Error(`Aggregate obligation ${aggregateId} was reused while resolving ${manifestId}`);
  }
  usedAggregateObligationIds.add(aggregateId);
}

function parseAggregateSelectorContract(observed) {
  if (observed.selector_contract && typeof observed.selector_contract === "object") {
    return observed.selector_contract;
  }
  if (typeof observed.selector_contract === "string") {
    try {
      const parsed = JSON.parse(observed.selector_contract);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw new Error("selector contract must be a JSON object");
      }
      return parsed;
    } catch (error) {
      throw new Error(`Aggregate obligation ${observed.obligation_id} has an invalid selector_contract: ${error.message}`);
    }
  }
  if (observed.selectors && typeof observed.selectors === "object") return observed.selectors;
  return null;
}

function runtimeSelectorCorrelationKey(obligation, selectors) {
  return stableStringify({
    domain: obligation.domain ?? null,
    subject_kind: obligation.subject_kind ?? null,
    subject_id: obligation.subject_id ?? null,
    required_event_kinds: uniqueSorted(obligation.required_event_kinds ?? []),
    runtime_selectors: runtimeSelectorContract(selectors),
  });
}

function runtimeSelectorContract(selectors) {
  return Object.fromEntries(
    Object.entries(selectors ?? {})
      .filter(([key]) => key !== "source_rule_ids")
      .sort(([left], [right]) => compareText(left, right))
      .map(([key, value]) => [key, normalizeSelectorValue(value)]),
  );
}

function normalizeSelectorValue(value) {
  if (Array.isArray(value)) {
    return value.map(normalizeSelectorValue).sort((left, right) => compareText(stableStringify(left), stableStringify(right)));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => compareText(left, right))
        .map(([key, nested]) => [key, normalizeSelectorValue(nested)]),
    );
  }
  return value;
}

function runtimeSelectorContractHash(selectors) {
  return hashText(stableStringify(runtimeSelectorContract(selectors)));
}

function staticStateForSource(sourceRuleId, formulaRequired, staticBySource) {
  const source = staticBySource.get(sourceRuleId);
  if (!formulaRequired) return { source_rule_id: sourceRuleId, state: "not-required", blockers: [] };
  if (!source) return { source_rule_id: sourceRuleId, state: "missing", blockers: ["formula-input-required-but-static-source-is-absent"] };
  if (source.static_gate_resolved) return { source_rule_id: sourceRuleId, state: "resolved", blockers: [] };
  return {
    source_rule_id: sourceRuleId,
    state: "unresolved",
    blockers: uniqueSorted(source.remaining_static_blockers ?? ["unclassified-static-formula-blocker"]),
  };
}

function classifyStatus(gates) {
  if (!gates.buildMatches) return "matching-build-evidence-missing";
  if (!gates.hasCandidate) return "no-candidate-evidence";
  if (gates.transferClass === "nontransfer") {
    return gates.sourceConservationComplete ? "proven-zero-transfer-source-owned" : "observed-nontransfer-awaiting-source-conservation";
  }
  if (gates.transferClass === "component-scoped") return "observed-component-scoped-awaiting-component-gates";
  if (gates.transferClass === "unresolved") return "observed-transfer-scope-unresolved";
  if (gates.transferClass === "non-outgoing") return "observed-non-outgoing-context";
  if (gates.transferClass === "missing") return "observed-typed-transfer-gate-missing";
  if (gates.providerObservations === 0) return "no-provider-recipient-evidence";
  if (gates.externalObservations === 0) return "observed-self-only-external-scope-unproven";
  if (gates.componentScalarProofPresent && !gates.componentScalarProviderOwnershipComplete) {
    return "observed-external-scope-awaiting-provider-ownership";
  }
  if (!gates.lifecycleComplete) return "observed-external-scope-awaiting-lifecycle";
  if (!gates.staticComplete) return "observed-external-scope-awaiting-static-model";
  if (!gates.formulaComplete) return "observed-external-scope-awaiting-formula-inputs";
  if (!gates.projectionComplete) return "observed-external-scope-awaiting-projection";
  if (!gates.packetConservationComplete) return "observed-external-scope-awaiting-packet-conservation";
  return "proven-promotable";
}

function buildSourceResults(obligations, staticBySource, workbenchModelsBySource) {
  const grouped = new Map();
  for (const obligation of obligations) {
    for (const sourceRuleId of obligation.source_rule_ids) {
      if (!grouped.has(sourceRuleId)) grouped.set(sourceRuleId, []);
      grouped.get(sourceRuleId).push(obligation);
    }
  }
  return [...grouped.entries()].map(([sourceRuleId, entries]) => {
    const staticSource = staticBySource.get(sourceRuleId);
    const statuses = countBy(entries, (entry) => entry.status);
    const blockers = uniqueSorted(entries.flatMap((entry) => entry.blockers));
    return {
      source_rule_id: sourceRuleId,
      source_id: staticSource?.source_id ?? null,
      source_name: staticSource?.source_name ?? entries[0]?.subject_name ?? sourceRuleId,
      manifest_obligations: entries.length,
      shared_model_keys: uniqueSorted(workbenchModelsBySource.get(sourceRuleId) ?? []),
      static_gate_resolved: staticSource?.static_gate_resolved ?? null,
      status: entries.every((entry) => entry.status === "proven-promotable") ? "proven-promotable" : "open",
      obligation_status_counts: statuses,
      blockers,
      obligation_ids: uniqueSorted(entries.map((entry) => entry.obligation_id)),
    };
  }).sort((left, right) => compareText(left.source_rule_id, right.source_rule_id));
}

function buildModelResults(workbench, obligationResults) {
  const bySource = new Map();
  for (const obligation of obligationResults) {
    for (const sourceRuleId of obligation.source_rule_ids) {
      if (!bySource.has(sourceRuleId)) bySource.set(sourceRuleId, []);
      bySource.get(sourceRuleId).push(obligation);
    }
  }
  const workbenchObligations = uniqueIndex(workbench.obligations ?? [], "obligation_id", "workbench obligation");
  return (workbench.model_groups ?? []).map((group) => {
    const runtimeObligations = uniqueBy(
      group.source_rule_ids.flatMap((sourceRuleId) => bySource.get(sourceRuleId) ?? []),
      (entry) => entry.obligation_id,
    );
    const componentObligations = group.obligation_ids.map((id) => workbenchObligations.get(id));
    if (componentObligations.some((entry) => !entry)) throw new Error(`Workbench group ${group.model_key} references an unknown obligation`);
    const statusCounts = countBy(runtimeObligations, (entry) => entry.status);
    const externalObserved = runtimeObligations.filter((entry) => entry.gates.external_provider_scope).length;
    const lifecycleClosed = runtimeObligations.filter((entry) => entry.gates.lifecycle).length;
    const formulaInputsClosed = runtimeObligations.filter((entry) => entry.gates.formula_inputs).length;
    const projectionClosed = runtimeObligations.filter((entry) => entry.gates.counterfactual_projection).length;
    const proofReceipts = group.proof_receipts ?? [];
    const proofStates = uniqueSorted(proofReceipts.map((receipt) => receipt.state));
    const hasProofReceipt = proofReceipts.length > 0;
    const stillRequiredRuntimeGates = uniqueSorted(
      proofReceipts.flatMap((receipt) => receipt.still_required_runtime_gates ?? []),
    );
    return {
      model_key: group.model_key,
      model_family: group.model_family,
      component_key: group.component_key,
      proof_contract: group.proof_contract,
      registry_only_proof_route: group.registry_only_proof_route === true,
      source_count: group.source_count,
      static_blocker_obligations: group.obligation_count,
      runtime_manifest_obligations: runtimeObligations.length,
      repeated_source_investigations_avoided: Math.max(0, group.source_count - 1),
      component_evidence_counts: group.component_evidence_counts,
      manual_component_binding_obligations: group.manual_component_binding_obligations,
      runtime_selector_obligations: group.runtime_selector_obligations,
      runtime_status_counts: statusCounts,
      runtime_progress: {
        external_provider_scope_observed: externalObserved,
        lifecycle_closed: lifecycleClosed,
        formula_inputs_closed: formulaInputsClosed,
        counterfactual_projection_closed: projectionClosed,
      },
      status: hasProofReceipt
        ? "shared-model-proof-received-runtime-open"
        : "shared-model-proof-open",
      blockers: uniqueSorted([
        ...(hasProofReceipt ? [] : ["shared-static-model-has-no-current-build-proof-receipt"]),
        ...stillRequiredRuntimeGates.map((gate) => `shared-proof-runtime-gate-open:${gate}`),
        ...runtimeObligations.flatMap((entry) => entry.blockers),
      ]),
      source_rule_ids: group.source_rule_ids,
      workbench_obligation_ids: group.obligation_ids,
      runtime_obligation_ids: runtimeObligations.map((entry) => entry.obligation_id).sort(compareText),
      proof_states: proofStates,
      still_required_runtime_gates: stillRequiredRuntimeGates,
      proof_receipt: hasProofReceipt ? proofReceipts : null,
    };
  }).sort((left, right) => right.source_count - left.source_count || compareText(left.model_key, right.model_key));
}

function summarize(context) {
  const { obligationResults, sourceResults, modelResults, runtimeEffectResults, counterfactualFrontierResults, manifest, aggregate, aggregatePresent, staticEvidence, workbench } = context;
  const exactRuntimeSubsets = runtimeEffectResults.filter((entry) =>
    entry.status === "runtime-attribution-promoted-exact-subset"
  );
  const exactRuntimeDamageEvents = exactRuntimeSubsets.reduce(
    (sum, entry) => sum + Number(entry.evidence?.exact_replay_damage_events ?? 0),
    0,
  );
  const deferredAmbiguousRuntimeEvents = exactRuntimeSubsets.reduce(
    (sum, entry) => sum + Number(entry.evidence?.ambiguous_provider_window_events ?? 0),
    0,
  );
  const historicalLeadOnlyObligations = obligationResults.filter((entry) =>
    (entry.historical_proof_leads?.length ?? 0) > 0 && entry.status !== "proven-promotable"
  ).length;
  const strictlyPromotableObligations = obligationResults.filter((entry) =>
    entry.status === "proven-promotable"
  ).length;
  const openObligations = obligationResults.filter((entry) => !isClosedStatus(entry.status)).length;
  return {
    raw_frontier_work_items: Number(manifest.summary?.frontier_work_items ?? obligationResults.length),
    manifest_indexed_obligations: Number(manifest.summary?.indexed_obligations ?? obligationResults.length),
    manifest_explicitly_unindexable_obligations: Number(
      manifest.summary?.explicitly_unindexable_obligations ?? manifest.summary?.explicitly_unindexable_work_items ?? 0,
    ),
    audited_obligations: obligationResults.length,
    audited_source_rules: sourceResults.length,
    strictly_promotable_obligations: strictlyPromotableObligations,
    proven_zero_transfer_obligations: obligationResults.filter((entry) => entry.status === "proven-zero-transfer-source-owned").length,
    open_obligations: openObligations,
    attribution_progress: {
      current_build_exact_conserved_effect_subsets: exactRuntimeSubsets.length,
      current_build_exact_conserved_damage_events: exactRuntimeDamageEvents,
      current_build_full_effect_families_resolved: runtimeEffectResults.filter((entry) =>
        entry.full_effect_family_resolved === true
      ).length,
      current_build_fully_promotable_manifest_obligations: strictlyPromotableObligations,
      current_build_pending_manifest_obligations: openObligations,
      historical_lead_only_manifest_obligations: historicalLeadOnlyObligations,
      deferred_ambiguous_provider_window_damage_events: deferredAmbiguousRuntimeEvents,
      current_build_counterfactual_frontier_open_effect_loci: counterfactualFrontierResults.length,
      current_build_counterfactual_exact_controlled_effect_loci: counterfactualFrontierResults.filter(
        (entry) => Number(entry.exact_recorded_inputs.controlled_groups ?? 0) > 0,
      ).length,
      current_build_counterfactual_exact_divergent_effect_loci: counterfactualFrontierResults.filter(
        (entry) => Number(entry.exact_recorded_inputs.divergent_output_groups ?? 0) > 0,
      ).length,
      current_build_counterfactual_run_scoped_player_provider_owned_effect_loci:
        counterfactualFrontierResults.filter(
          (entry) => entry.provider_ownership_evidence?.run_scoped_player_ownership_proven === true,
        ).length,
      current_build_counterfactual_integer_transform_constrained_effect_loci:
        counterfactualFrontierResults.filter(
          (entry) => entry.integer_transform_constraints?.exact_transform_proven === false,
        ).length,
      unresolved_evidence_hidden: 0,
    },
    obligation_status_counts: countBy(obligationResults, (entry) => entry.status),
    transfer_class_counts: countBy(obligationResults, (entry) => entry.transfer_class),
    correlation_match_counts: countBy(obligationResults, (entry) => entry.correlation_match?.kind ?? "missing"),
    gate_pass_counts: Object.fromEntries(Object.keys(closureGateDescriptions()).map((gate) => [gate, obligationResults.filter((entry) => entry.gates[gate]).length])),
    packet_evidence: {
      aggregate_input_present: aggregatePresent,
      total_events: Number(aggregate.total_events ?? 0),
      candidate_coverage_complete: Number(aggregate.summary?.candidate_event_coverage_complete ?? 0),
      external_provider_scope_observed: obligationResults.filter((entry) => entry.gates.external_provider_scope).length,
      lifecycle_closed: obligationResults.filter((entry) => entry.gates.lifecycle).length,
      formula_input_snapshots_present: obligationResults.filter((entry) => entry.evidence.formula_input_snapshots > 0).length,
      projections_present: obligationResults.filter((entry) => entry.evidence.projected_integer_events + entry.evidence.projected_rational_events > 0).length,
      packet_damage_rows_present: obligationResults.filter((entry) => entry.evidence.packet_damage_rows > 0).length,
    },
    packet_observed_runtime_effects: runtimeEffectResults.length,
    packet_observed_runtime_external_candidates: runtimeEffectResults.filter((entry) => EXTERNAL_RUNTIME_EFFECT_STATUSES.has(entry.status)).length,
    packet_observed_runtime_attribution_promoted_exact_subset: runtimeEffectResults.filter((entry) => entry.status === "runtime-attribution-promoted-exact-subset").length,
    packet_observed_runtime_model_ready_awaiting_strict_conservation: runtimeEffectResults.filter((entry) => entry.status === "runtime-model-ready-awaiting-strict-conservation").length,
    packet_observed_runtime_external_open: runtimeEffectResults.filter((entry) => entry.status === "runtime-external-open").length,
    packet_observed_runtime_non_outgoing_context: runtimeEffectResults.filter((entry) => entry.status === "packet-observed-non-outgoing-context").length,
    packet_observed_runtime_non_external: runtimeEffectResults.filter((entry) => entry.status === "packet-observed-non-external").length,
    packet_observed_runtime_status_counts: countBy(runtimeEffectResults, (entry) => entry.status),
    counterfactual_frontier_effect_loci: counterfactualFrontierResults.length,
    counterfactual_frontier_status_counts: countBy(counterfactualFrontierResults, (entry) => entry.status),
    counterfactual_frontier_run_scoped_player_provider_owned_effect_loci:
      counterfactualFrontierResults.filter(
        (entry) => entry.provider_ownership_evidence?.run_scoped_player_ownership_proven === true,
      ).length,
    counterfactual_frontier_integer_transform_constrained_effect_loci:
      counterfactualFrontierResults.filter(
        (entry) => entry.integer_transform_constraints?.exact_transform_proven === false,
      ).length,
    static_formula_sources: Number(staticEvidence.summary?.sources ?? staticEvidence.sources?.length ?? 0),
    static_formula_sources_resolved: Number(staticEvidence.summary?.static_gates_resolved ?? 0),
    shared_formula_models: modelResults.length,
    shared_formula_models_closed: modelResults.filter((entry) => entry.status === "proved").length,
    shared_formula_models_open: modelResults.filter((entry) => entry.status !== "proved").length,
    shared_formula_models_proof_received_runtime_open: modelResults.filter((entry) => entry.status === "shared-model-proof-received-runtime-open").length,
    shared_formula_models_offline_proven_runtime_open: modelResults.filter((entry) => entry.proof_states?.includes(OFFLINE_FORMULA_PROOF_STATE)).length,
    shared_formula_models_canonical_runtime_input_route_proven_runtime_open: modelResults.filter((entry) => entry.proof_states?.includes(CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE)).length,
    registry_only_proof_route_models: modelResults.filter((entry) => entry.registry_only_proof_route).length,
    blocker_obligations_collapsed_into_models: Number(workbench.summary?.blocker_obligations ?? 0),
    repeated_investigations_avoided: Number(workbench.summary?.source_investigations_avoided_if_proved_by_group ?? 0),
    closure_audit_optimization_complete: true,
    strict_rdps_proof_complete: obligationResults.every((entry) => isClosedStatus(entry.status)) &&
      counterfactualFrontierResults.length === 0,
    hidden_omissions: 0,
  };
}

function closureGateDescriptions() {
  return {
    matching_build: "The manifest, captures, and observed event build must match exactly.",
    candidate_evidence: "At least one matching canonical packet event must exist.",
    static_formula_model: "Every required formula source must have a resolved typed static model.",
    transfer_eligibility: "The exact-build typed transfer gate must explicitly allow external counterfactual attribution.",
    external_provider_scope: "A provider distinct from the recipient must be packet-observed for transferable rDPS.",
    lifecycle: "Apply and terminal lifecycle states must be observed with no ambiguous removal or provider window.",
    formula_inputs: "Every required event-time formula operand snapshot must be present and complete.",
    counterfactual_projection: "A valid provider-owned counterfactual must project externally with no invalid or excluded events.",
    packet_conservation: "Projected contribution must reconcile against preserved packet damage rows.",
  };
}

function buildPartySkillFrontier(document, documentPath, buildId) {
  if (Number(document?.schema_version) !== 2 ||
    document?.generated_by !== "tools/bpsr-party-skill-static-closure.mjs" ||
    String(document?.game_build) !== String(buildId) ||
    document?.content_sha256 !== contentHash(document) ||
    document?.policy?.exact_numeric_skill_effect_buff_ids_and_build_are_authoritative !== true ||
    document?.policy?.localized_names_and_descriptions_are_discovery_evidence_only !== true ||
    document?.policy?.remote_player_cast_packets_required !== false ||
    document?.policy?.remote_player_cast_packets_treated_as_zero !== false ||
    document?.policy?.remote_player_cast_packets_synthesized !== false ||
    document?.policy?.unresolved_skill_to_buff_edges_preserved !== true ||
    document?.policy?.reviewed_candidate_links_are_exact_runtime_edges !== false ||
    document?.policy?.provider_rdps_credit_allowed !== false ||
    document?.runtime_decision?.provider_rdps_credit_allowed !== false ||
    document?.runtime_decision?.runtime_catalog_promotion_allowed !== false ||
    document?.runtime_decision?.ui_rdps_display_allowed !== false ||
    document?.runtime_decision?.ordinary_damage_totals_unchanged !== true ||
    Number(document?.summary?.hidden_omissions) !== 0 ||
    Number(document?.summary?.provider_rdps_credit_allowed_rows) !== 0) {
    throw new Error("Party-skill static closure is not an exact-build fail-closed frontier");
  }
  if (Number(document?.summary?.skill_candidates) !== document?.skill_candidates?.length ||
    Number(document?.summary?.rogue_party_entry_candidates) !==
      document?.rogue_party_entry_candidates?.length) {
    throw new Error("Party-skill static closure candidate counts are inconsistent");
  }
  const skillResults = (document.skill_candidates ?? [])
    .filter((entry) => entry?.rdps_relevant_candidate === true)
    .map((entry) => {
      const skillId = Number(entry?.skill_id);
      if (!Number.isSafeInteger(skillId) || skillId <= 0 ||
        entry?.runtime_formula_authority !== false ||
        entry?.provider_rdps_credit_allowed !== false ||
        !Array.isArray(entry?.proof_obligations) || entry.proof_obligations.length === 0 ||
        entry?.reviewed_candidate_skill_to_buff_links?.some((link) =>
          link?.exact_skill_to_buff_edge_proven !== false ||
          link?.runtime_attribution_enabled !== false
        ) ||
        entry?.exact_build_party_talent_route_evidence?.some((route) =>
          route?.transformation_opcode_semantics_authoritative !== false ||
          route?.runtime_formula_authority !== false
        )) {
        throw new Error(`Unsafe party-skill static frontier row ${String(entry?.skill_id)}`);
      }
      return {
        skill_id: skillId,
        localized_name_evidence: entry.localized_name_evidence ?? null,
        support_categories: uniqueSorted(entry.support_categories ?? []),
        presentation_state: String(entry.presentation_state),
        skill_to_buff_graph_state: String(entry.skill_to_buff_graph_state),
        exact_reviewed_buff_or_status_ids: uniqueSortedNumbers(
          entry.exact_reviewed_buff_or_status_ids ?? [],
        ),
        reviewed_candidate_buff_or_status_ids: uniqueSortedNumbers(
          (entry.reviewed_candidate_skill_to_buff_links ?? []).map((link) => link.buff_id),
        ),
        exact_build_party_talent_ids: uniqueSortedNumbers(
          (entry.exact_build_party_talent_route_evidence ?? []).map((route) => route.talent_id),
        ),
        proof_obligations: uniqueSorted(entry.proof_obligations),
        formula_authority: false,
        runtime_authority: false,
        ui_display_authority: false,
        provider_rdps_credit_allowed: false,
      };
    }).sort((left, right) => left.skill_id - right.skill_id);
  const rogueEntryResults = (document.rogue_party_entry_candidates ?? [])
    .filter((entry) => entry?.rdps_relevant_candidate === true)
    .map((entry) => {
      const entryId = Number(entry?.entry_id);
      const rootBuffId = Number(entry?.exact_root_buff_id);
      if (!Number.isSafeInteger(entryId) || entryId <= 0 ||
        !Number.isSafeInteger(rootBuffId) || rootBuffId <= 0 ||
        entry?.runtime_formula_authority !== false ||
        entry?.provider_rdps_credit_allowed !== false ||
        entry?.exact_entry_to_root_buff_edge?.target_present !== true ||
        !Array.isArray(entry?.proof_obligations) || entry.proof_obligations.length === 0 ||
        entry?.candidate_child_buff_family?.some((child) =>
          child?.exact_runtime_edge_proven !== false
        )) {
        throw new Error(`Unsafe party-entry static frontier row ${String(entry?.entry_id)}`);
      }
      return {
        entry_id: entryId,
        localized_name_evidence: entry.localized_name_evidence ?? null,
        support_categories: uniqueSorted(entry.support_categories ?? []),
        exact_root_buff_id: rootBuffId,
        candidate_child_buff_ids: uniqueSortedNumbers(
          (entry.candidate_child_buff_family ?? []).map((child) => child.buff_id),
        ),
        proof_obligations: uniqueSorted(entry.proof_obligations),
        formula_authority: false,
        runtime_authority: false,
        ui_display_authority: false,
        provider_rdps_credit_allowed: false,
      };
    }).sort((left, right) => left.entry_id - right.entry_id);
  if (skillResults.length !== Number(document?.summary?.rdps_relevant_skill_candidates) ||
    rogueEntryResults.length !==
      Number(document?.summary?.rdps_relevant_rogue_party_entry_candidates) ||
    new Set(skillResults.map((entry) => entry.skill_id)).size !== skillResults.length ||
    new Set(rogueEntryResults.map((entry) => entry.entry_id)).size !== rogueEntryResults.length) {
    throw new Error("Party-skill static frontier omitted or duplicated an rDPS candidate");
  }
  return {
    proof_state: "exact-build-static-party-frontier-runtime-proof-open",
    input: fileDescriptor(documentPath),
    skill_results: skillResults,
    rogue_entry_results: rogueEntryResults,
    complete: [...skillResults, ...rogueEntryResults]
      .every((entry) => entry.provider_rdps_credit_allowed),
    packet_obligations_fabricated: 0,
    unresolved_evidence_hidden: 0,
  };
}

function resolvePartyEffectAffectedEntityRole({
  statusEvents,
  supportCategories,
  identityEvidence,
  externalAffectedEntityPartyMembershipProvenForEveryStatusEvent,
  actorRelation,
  targetRelation,
  damageActionEdgeSummary,
  requirePartyMembershipForRecipientRole = false,
}) {
  const identityComplete = statusEvents > 0 &&
    Number(identityEvidence?.affected_entity_status_events) === statusEvents &&
    Number(identityEvidence?.affected_entity_identity_unresolved_events) === 0;
  const targetRelationshipObserved = Number(targetRelation?.event_count) > 0 &&
    Number(damageActionEdgeSummary?.effect_target_as_damage_target_edges) > 0 &&
    Number(damageActionEdgeSummary?.effect_target_as_damage_target_event_references) ===
      Number(targetRelation?.event_count);
  const actorRelationshipObserved = Number(actorRelation?.event_count) > 0 &&
    Number(damageActionEdgeSummary?.effect_target_as_damage_actor_edges) > 0 &&
    Number(damageActionEdgeSummary?.effect_target_as_damage_actor_event_references) ===
      Number(actorRelation?.event_count);
  const targetVulnerabilityCandidate = supportCategories.includes("external-target-vulnerability");
  if (identityComplete && targetVulnerabilityCandidate && targetRelationshipObserved) {
    return {
      proven: true,
      resolution: "damage-target-allegiance-neutral",
      requires_party_membership: false,
    };
  }
  const partyRecipientCandidate = supportCategories.some((category) => new Set([
    "party-action-opportunity",
    "party-defensive-support",
    "party-healing",
    "party-offensive-stat",
    "party-resource-support",
  ]).has(category));
  const recipientRelationshipCandidate = partyRecipientCandidate && !targetVulnerabilityCandidate;
  if (identityComplete && recipientRelationshipCandidate && actorRelationshipObserved &&
    (!requirePartyMembershipForRecipientRole ||
      externalAffectedEntityPartyMembershipProvenForEveryStatusEvent)) {
    return {
      proven: true,
      resolution: requirePartyMembershipForRecipientRole
        ? "party-recipient-damage-actor"
        : "damage-actor-allegiance-neutral",
      requires_party_membership: requirePartyMembershipForRecipientRole,
    };
  }
  return {
    proven: false,
    resolution: "unresolved",
    requires_party_membership:
      requirePartyMembershipForRecipientRole && recipientRelationshipCandidate,
  };
}

function buildPartyEffectWindowFrontier(
  document,
  documentPath,
  partySkillFrontier,
  supportEffectFrontierResults,
  providerOwnershipIndex,
  statusEventSeasonContextIndex,
  seasonStateMutationReceipt,
  partyHasteStackingReceipt,
  partyHasteActionReceipt,
  imagineFormulaReceiptIndex,
  buildId,
) {
  const auditSchemaVersion = Number(document?.schema_version);
  if (auditSchemaVersion !== 8 ||
    document?.generated_by !== "rlogs-bpsr-party-effect-window-audit" ||
    String(document?.game_build) !== String(buildId) ||
    document?.policy?.exact_numeric_effect_ids_and_build_are_authoritative !== true ||
    document?.policy?.localized_names_are_runtime_keys !== false ||
    document?.policy?.remote_player_cast_packets_required !== false ||
    document?.policy?.remote_player_cast_packets_treated_as_zero !== false ||
    document?.policy?.remote_player_cast_packets_synthesized !== false ||
    document?.policy?.status_rows_without_provider_are_preserved !== true ||
    document?.policy?.actor_identity_is_event_time_canonical_evidence_only !== true ||
    document?.policy?.player_identity_is_party_membership_authority !== false ||
    (auditSchemaVersion >= 5 &&
      document?.policy?.explicit_party_roster_evidence_consumed !== true) ||
    (auditSchemaVersion >= 5 &&
      document?.policy?.party_roster_lifecycle_route_coverage_proven !== false) ||
    (auditSchemaVersion >= 6 &&
      (document?.policy?.exact_build_team_id_attribute_evidence_consumed !== true ||
        String(document?.policy?.team_attribute_interpretation_build) !== String(buildId) ||
        Number(document?.policy?.team_id_attribute_id) !== 194 ||
        Number(document?.policy?.team_member_count_attribute_id) !== 195 ||
        document?.policy?.team_attribute_protocol_event_coverage_proven !== false ||
        document?.policy
          ?.matching_last_observed_team_ids_grant_party_membership_authority !== false)) ||
    (auditSchemaVersion >= 7 &&
      (String(document?.policy?.fight_source_enum_build) !== String(buildId) ||
        document?.policy?.fight_source_type_identity_exact_build_gated !== true ||
        document?.policy?.packet_origin_edges_are_skill_to_buff_edges !== false ||
        document?.policy?.packet_origin_edges_are_provider_ownership_authority !== false ||
        document?.policy?.packet_origin_edges_are_formula_authority !== false)) ||
    (auditSchemaVersion >= 8 &&
      (document?.policy?.status_source_to_effect_target_lifecycle_is_preserved !== true ||
        document?.policy?.status_source_is_provider_ownership_authority !== false ||
        document?.policy?.effect_target_role_is_allegiance_neutral !== true ||
        document?.policy
          ?.effect_target_damage_actor_and_damage_target_edges_are_separate !== true ||
        document?.policy?.damage_action_edges_preserve_actor_ability_and_target !== true ||
        document?.policy?.damage_action_edges_are_causal_or_formula_authority !== false)) ||
    document?.policy
      ?.damage_links_preserve_affected_entity_as_actor_and_as_target !== true ||
    document?.policy?.affected_entity_is_assumed_friendly !== false ||
    document?.policy?.affected_entity_is_assumed_enemy !== false ||
    document?.policy?.timeline_presence_is_formula_authority !== false ||
    document?.policy?.provider_rdps_credit_authorized !== false ||
    document?.policy?.runtime_promotion_allowed !== false ||
    document?.policy?.ui_display_allowed !== false) {
    throw new Error("Party-effect window audit is not an exact-build allegiance-neutral frontier");
  }
  const effects = document.effects ?? [];
  const windows = document.windows ?? [];
  if (!Array.isArray(effects) || !Array.isArray(windows) ||
    effects.length !== Number(document?.summary?.party_effects_in_frontier) ||
    windows.length !== Number(document?.summary?.windows) ||
    Number(document?.summary?.rlogs_verified) <= 0 ||
    Number(document?.summary?.canonical_events) < Number(document?.summary?.damage_events) ||
    Number(document?.summary?.remote_cast_rows_synthesized) !== 0 ||
    Number(document?.summary?.provider_rdps_credit_authorized_effects) !== 0 ||
    (auditSchemaVersion >= 6 &&
      (!Number.isSafeInteger(Number(document?.summary?.team_id_attribute_events)) ||
        Number(document.summary.team_id_attribute_events) < 0 ||
        !Number.isSafeInteger(Number(document?.summary?.team_id_attribute_positive_values)) ||
        !Number.isSafeInteger(Number(document?.summary?.team_id_attribute_clear_values)) ||
        !Number.isSafeInteger(Number(document?.summary?.team_id_attribute_malformed_values)) ||
        Number(document.summary.team_id_attribute_positive_values) +
          Number(document.summary.team_id_attribute_clear_values) +
        Number(document.summary.team_id_attribute_malformed_values) !==
          Number(document.summary.team_id_attribute_events) ||
        !Array.isArray(document?.summary?.team_id_attribute_malformed_examples) ||
        document.summary.team_id_attribute_malformed_examples.length > 16 ||
        document.summary.team_id_attribute_malformed_examples.length >
          Number(document.summary.team_id_attribute_malformed_values) ||
        document.summary.team_id_attribute_malformed_examples.some((example) =>
          typeof example?.session_id !== "string" || example.session_id.length === 0 ||
          !Number.isSafeInteger(Number(example?.sequence)) || Number(example.sequence) < 0 ||
          !Number.isSafeInteger(Number(example?.observed_micros)) ||
          Number(example.observed_micros) < 0 ||
          !/^\d+$/.test(String(example?.actor_id ?? "")) ||
          !/^-?\d+$/.test(String(example?.entity_uuid ?? "")) ||
          !Array.isArray(example?.raw_value) || example.raw_value.some((value) =>
            !Number.isSafeInteger(Number(value)) || Number(value) < 0 || Number(value) > 255)) ||
        !Number.isSafeInteger(Number(document?.summary?.team_member_count_attribute_events)) ||
        Number(document.summary.team_member_count_attribute_events) < 0)) ||
    String(document?.inputs?.party_closure_sha256 ?? "") !==
      String(partySkillFrontier?.input?.sha256 ?? "") ||
    Number(document?.inputs?.party_closure_bytes) !== Number(partySkillFrontier?.input?.bytes)) {
    throw new Error("Party-effect window audit summary or static-frontier identity is inconsistent");
  }
  const damageActionTimeline = auditSchemaVersion >= 8
    ? validatePartyDamageActionWindows(windows, document)
    : { byEffect: new Map(), edgeCount: 0, actorEdgeCount: 0, targetEdgeCount: 0 };
  const partyRlogInputs = normalizePartyEffectRlogInputs(document?.inputs?.rlogs);
  const effectResults = effects.map((entry) => {
    const effectId = Number(entry?.effect_id);
    const formulaReceipt = supportEffectFrontierResults.find(
      (proof) => Number(proof?.effect_id) === effectId,
    ) ?? null;
    const actorRelation = entry?.affected_entity_damage_actions;
    const targetRelation = entry?.damage_actions_targeting_affected_entity;
    const identityEvidence = entry?.identity_evidence;
    const providerOwnership = summarizePartyEffectProviderOwnership(
      providerOwnershipIndex.get(effectId) ?? null,
      entry,
      partyRlogInputs,
    );
    const seasonContext = summarizePartyEffectSeasonContext(
      statusEventSeasonContextIndex.get(effectId) ?? null,
      entry,
      partyRlogInputs,
      seasonStateMutationReceipt,
    );
    const stackingFrontier = effectId === 31602 && partyHasteStackingReceipt
      ? structuredClone(partyHasteStackingReceipt)
      : null;
    const actionSpeedCounterfactualFrontier = effectId === 31602 && partyHasteActionReceipt
      ? structuredClone(partyHasteActionReceipt)
      : null;
    const imagineFormula = imagineFormulaReceiptIndex.get(effectId)
      ? structuredClone(imagineFormulaReceiptIndex.get(effectId))
      : null;
    const observedOriginEdges = normalizePartyObservedOriginEdges(
      entry,
      effectId,
      auditSchemaVersion,
      buildId,
    );
    if (!Number.isSafeInteger(effectId) || effectId <= 0 ||
      (entry?.exact_static_edge !== true && entry?.reviewed_candidate_edge !== true) ||
      entry?.provider_rdps_credit_authorized !== false ||
      !Array.isArray(entry?.source_skill_ids) || !Array.isArray(entry?.source_entry_ids) ||
      !Array.isArray(entry?.support_categories) ||
      !Number.isSafeInteger(Number(entry?.status_events)) || Number(entry.status_events) < 0 ||
      !Number.isSafeInteger(Number(entry?.status_events_with_source)) ||
      !Number.isSafeInteger(Number(entry?.status_events_without_source)) ||
      Number(entry.status_events_with_source) + Number(entry.status_events_without_source) !==
        Number(entry.status_events) ||
      !isValidPartyDamageRelation(actorRelation) || !isValidPartyDamageRelation(targetRelation) ||
      !isValidPartyIdentityEvidence(identityEvidence, auditSchemaVersion) ||
      !Array.isArray(entry?.reported_duration_millis) ||
      !Array.isArray(entry?.reported_status_levels) || !Array.isArray(entry?.reported_stacks) ||
      !Array.isArray(entry?.reported_counts)) {
      throw new Error(`Unsafe party-effect window frontier row ${String(entry?.effect_id)}`);
    }
    const statusEvents = Number(entry.status_events);
    const affectedEntityIdentityProvenForEveryStatusEvent =
      Number(identityEvidence.affected_entity_status_events) === statusEvents &&
      Number(identityEvidence.affected_entity_identity_unresolved_events) === 0 &&
      Number(identityEvidence.affected_entity_player_identity_events) +
        Number(identityEvidence.affected_entity_non_player_identity_events) === statusEvents;
    const affectedEntityPlayerIdentityProvenForEveryStatusEvent =
      affectedEntityIdentityProvenForEveryStatusEvent &&
      Number(identityEvidence.affected_entity_player_identity_events) === statusEvents;
    const externalAffectedEntityPartyMembershipProvenForEveryStatusEvent =
      Number(identityEvidence.external_source_affected_status_events) === 0 ||
      Number(identityEvidence.party_membership_proven_status_events) ===
        Number(identityEvidence.external_source_affected_status_events);
    const supportCategories = uniqueSorted(entry.support_categories);
    const damageActionEdgeSummary = auditSchemaVersion >= 8
      ? structuredClone(damageActionTimeline.byEffect.get(effectId) ??
        emptyPartyDamageActionEdgeSummary())
      : null;
    const affectedEntityRole = resolvePartyEffectAffectedEntityRole({
      statusEvents,
      supportCategories,
      identityEvidence,
      externalAffectedEntityPartyMembershipProvenForEveryStatusEvent,
      actorRelation,
      targetRelation,
      damageActionEdgeSummary,
    });
    return {
      effect_id: effectId,
      exact_static_edge: entry.exact_static_edge === true,
      reviewed_candidate_edge: entry.reviewed_candidate_edge === true,
      source_skill_ids: uniqueSortedNumbers(entry.source_skill_ids),
      source_entry_ids: uniqueSortedNumbers(entry.source_entry_ids),
      support_categories: supportCategories,
      status_events: Number(entry.status_events),
      status_events_with_source: Number(entry.status_events_with_source),
      status_events_without_source: Number(entry.status_events_without_source),
      unique_source_actor_count: entry.unique_source_actor_ids?.length ?? 0,
      unique_affected_entity_actor_count: entry.unique_affected_entity_actor_ids?.length ?? 0,
      observed_origin_edges: observedOriginEdges,
      lifecycle_counts: structuredClone(entry.lifecycle_counts ?? {}),
      windows_closed: Number(entry.windows_closed ?? 0),
      windows_open_at_log_end: Number(entry.windows_open_at_log_end ?? 0),
      orphan_lifecycle_windows: Number(entry.orphan_lifecycle_windows ?? 0),
      affected_entity_damage_actions: summarizePartyDamageRelation(actorRelation),
      damage_actions_targeting_affected_entity: summarizePartyDamageRelation(targetRelation),
      damage_action_edge_summary: damageActionEdgeSummary,
      reported_duration_millis: uniqueSortedNumbers(entry.reported_duration_millis),
      reported_status_levels: uniqueSortedNumbers(entry.reported_status_levels),
      reported_stacks: uniqueSortedNumbers(entry.reported_stacks),
      reported_counts: uniqueSortedNumbers(entry.reported_counts),
      identity_evidence: summarizePartyIdentityEvidence(identityEvidence, auditSchemaVersion),
      affected_entity_identity_proven_for_every_status_event:
        affectedEntityIdentityProvenForEveryStatusEvent,
      affected_entity_player_identity_proven_for_every_status_event:
        affectedEntityPlayerIdentityProvenForEveryStatusEvent,
      external_affected_entity_party_membership_proven_for_every_status_event:
        externalAffectedEntityPartyMembershipProvenForEveryStatusEvent,
      provider_ownership: providerOwnership,
      provider_ownership_proven_for_every_sourced_status_event:
        providerOwnership?.provider_ownership_proven_for_every_sourced_status_event === true,
      provider_ownership_proven_for_every_status_event:
        providerOwnership?.provider_ownership_proven_for_every_status_event === true,
      status_event_season_context: seasonContext,
      event_time_season_context_proven_for_every_status_event:
        seasonContext?.event_time_season_context_proven_for_every_status_event === true,
      exact_effect_to_stat_coefficient_proven:
        formulaReceipt?.mechanic === "party-haste-percent-status-coefficient" &&
        formulaReceipt?.exact_stat_transform_proven === true,
      effect_to_stat_coefficient: formulaReceipt?.mechanic ===
        "party-haste-percent-status-coefficient" ? {
          proof: structuredClone(formulaReceipt.proof),
          attribute_ids: structuredClone(formulaReceipt.changed_attribute_ids),
          raw_additive_coefficient_units: formulaReceipt.exact_raw_additive_coefficient_units,
          exact_origin: structuredClone(formulaReceipt.exact_origin),
          apply_occurrences: formulaReceipt.apply_occurrences,
          remove_occurrences: formulaReceipt.remove_occurrences,
          independent_run_contexts: formulaReceipt.independent_run_contexts,
          raw_unit_interpretation_authority: false,
        } : null,
      stacking_frontier: stackingFrontier,
      action_speed_counterfactual_frontier: actionSpeedCounterfactualFrontier,
      imagine_formula: imagineFormula,
      affected_entity_role_proven: affectedEntityRole.proven,
      affected_entity_role_resolution: affectedEntityRole.resolution,
      affected_entity_role_requires_party_membership:
        affectedEntityRole.requires_party_membership,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
      blockers: Number(entry.status_events) > 0
        ? [
          ...(providerOwnership?.provider_ownership_proven_for_every_status_event === true
            ? []
            : ["provider-ownership-for-every-status-event-open"]),
          ...(affectedEntityIdentityProvenForEveryStatusEvent
            ? []
            : ["affected-entity-event-time-identity-open"]),
          ...(affectedEntityRole.requires_party_membership &&
            !externalAffectedEntityPartyMembershipProvenForEveryStatusEvent
            ? ["event-time-party-membership-for-external-affected-entities-open"]
            : []),
          ...(affectedEntityRole.proven
            ? []
            : ["affected-entity-role-open"]),
          ...(formulaReceipt?.mechanic === "party-haste-percent-status-coefficient" &&
            seasonContext?.event_time_season_context_proven_for_every_status_event !== true
            ? [seasonContext
                ?.every_status_event_has_prior_continuous_monitor_context_candidate === true
              ? seasonContext
                  ?.every_continuous_monitor_candidate_has_classified_gap_routes === true
                ? seasonContext?.season_state_mutation_surface
                    ?.direct_literal_lua_writer_surface_complete === true
                  ? seasonContext?.season_state_mutation_surface
                      ?.normal_reconnect_preserves_season_state_by_static_control_flow === true
                    ? "continuous-monitor-season-context-normal-reconnect-preserves-state-awaiting-explicit-logout-exclusion-and-promoted-protocol-coverage"
                    : "continuous-monitor-season-context-direct-writers-proven-awaiting-intervening-clear-exclusion-and-promoted-protocol-coverage"
                  : "continuous-monitor-season-context-gap-routes-classified-awaiting-season-state-mutation-and-promoted-protocol-coverage"
                : "continuous-monitor-season-context-awaiting-protocol-event-coverage"
              : "event-time-season-context-for-status-events-open"]
            : []),
          ...(imagineFormula
            ? [imagineFormulaBlocker(effectId, imagineFormula)]
            : formulaReceipt?.mechanic === "party-haste-percent-status-coefficient"
              ? [stackingFrontier
                ? actionSpeedCounterfactualFrontier
                  ? "stacking-arbitration-unobserved-static-integer-semantics-open"
                  : "stacking-arbitration-unobserved-static-integer-semantics-and-downstream-operation-order-rounding-open"
                : "stacking-arbitration-operation-order-and-downstream-rounding-open"]
              : ["exact-effect-magnitude-operation-order-stacking-and-rounding-open"]),
          ...(formulaReceipt?.mechanic === "party-haste-percent-status-coefficient" &&
            actionSpeedCounterfactualFrontier
            ? ["native-action-speed-operation-order-proven-awaiting-bit-equivalent-action-time-input-and-temporary-term-replay-action-opportunity-packet-clock-and-conservation"]
            : []),
          ...(imagineFormula?.nontransfer_routing_marker_proven === true
            ? []
            : ["counterfactual-damage-projection-and-conservation-open"]),
        ]
        : ["exact-build-status-lifecycle-not-observed-in-reviewed-cohort"],
    };
  }).sort((left, right) => left.effect_id - right.effect_id);
  if (new Set(effectResults.map((entry) => entry.effect_id)).size !== effectResults.length ||
    effectResults.filter((entry) => entry.status_events > 0).length !==
      Number(document?.summary?.party_effects_observed) ||
    effectResults.reduce((sum, entry) => sum + entry.status_events, 0) !==
      Number(document?.summary?.party_status_events)) {
    throw new Error("Party-effect window audit omitted or duplicated effect evidence");
  }
  return {
    proof_state: "exact-build-canonical-party-effect-windows-identities-and-neutral-action-links-observed-semantics-open",
    input: fileDescriptor(documentPath),
    cohort: {
      rlogs_verified: Number(document.summary.rlogs_verified),
      canonical_events: Number(document.summary.canonical_events),
      damage_events: Number(document.summary.damage_events),
      cast_events_observed: Number(document.summary.cast_events_observed),
      party_roster_full_snapshot_events:
        Number(document.summary.party_roster_full_snapshot_events ?? 0),
      party_roster_members_observed_events:
        Number(document.summary.party_roster_members_observed_events ?? 0),
      party_roster_member_left_events:
        Number(document.summary.party_roster_member_left_events ?? 0),
      party_roster_dissolved_events:
        Number(document.summary.party_roster_dissolved_events ?? 0),
      team_id_attribute_events:
        Number(document.summary.team_id_attribute_events ?? 0),
      team_id_attribute_positive_values:
        Number(document.summary.team_id_attribute_positive_values ?? 0),
      team_id_attribute_clear_values:
        Number(document.summary.team_id_attribute_clear_values ?? 0),
      team_id_attribute_malformed_values:
        Number(document.summary.team_id_attribute_malformed_values ?? 0),
      team_id_attribute_malformed_examples:
        structuredClone(document.summary.team_id_attribute_malformed_examples ?? []),
      team_member_count_attribute_events:
        Number(document.summary.team_member_count_attribute_events ?? 0),
    },
    remote_cast_rows_synthesized: Number(document.summary.remote_cast_rows_synthesized),
    fight_source_enum_build: auditSchemaVersion >= 7
      ? String(document.policy.fight_source_enum_build)
      : null,
    packet_origin_edges_are_skill_to_buff_edges: false,
    packet_origin_edges_are_provider_ownership_authority: false,
    packet_origin_edges_are_formula_authority: false,
    affected_entity_allegiance_assumed: false,
    window_damage_action_edges: damageActionTimeline.edgeCount,
    window_damage_action_actor_edges: damageActionTimeline.actorEdgeCount,
    window_damage_action_target_edges: damageActionTimeline.targetEdgeCount,
    provider_ownership_proven_effects: effectResults
      .filter((entry) => entry.provider_ownership_proven_for_every_status_event).length,
    effect_results: effectResults,
    complete: effectResults.every((entry) => entry.provider_rdps_credit_allowed),
    provider_rdps_credit_authorized_effects: 0,
    unresolved_evidence_hidden: 0,
  };
}

function normalizePartyEffectRlogInputs(inputs) {
  if (!Array.isArray(inputs) || inputs.length === 0) {
    throw new Error("Party-effect window audit has no exact RLOG inputs");
  }
  const result = new Map();
  for (const input of inputs) {
    const label = path.basename(String(input?.path ?? "")).toLowerCase();
    const bytes = Number(input?.bytes);
    const sha256 = String(input?.sha256 ?? "").toLowerCase();
    if (!label || !Number.isSafeInteger(bytes) || bytes <= 0 ||
      !/^[0-9a-f]{64}$/.test(sha256) || result.has(label)) {
      throw new Error("Party-effect window audit RLOG provenance is incomplete or duplicated");
    }
    result.set(label, { bytes, sha256: `sha256:${sha256}` });
  }
  return result;
}

function summarizePartyEffectProviderOwnership(ownership, entry, partyRlogInputs) {
  if (!ownership) return null;
  const ownershipRlogs = ownership.input_rlogs;
  const exactCohortMatch = ownershipRlogs instanceof Map &&
    ownershipRlogs.size === partyRlogInputs.size &&
    [...partyRlogInputs.entries()].every(([label, expected]) => {
      const actual = ownershipRlogs.get(label);
      return Number(actual?.bytes) === expected.bytes &&
        String(actual?.sha256 ?? "").toLowerCase() === expected.sha256;
    });
  const sourcedStatusEvents = Number(entry?.status_events_with_source);
  const statusEventsWithoutSource = Number(entry?.status_events_without_source);
  const exactEventCountMatch = Number(ownership.selected_status_events) === sourcedStatusEvents;
  const providerOwnershipProvenForEverySourcedStatusEvent = exactCohortMatch &&
    exactEventCountMatch && ownership.run_scoped_player_ownership_proven === true;
  const providerOwnershipProvenForEveryStatusEvent =
    providerOwnershipProvenForEverySourcedStatusEvent && statusEventsWithoutSource === 0;
  return {
    proof: structuredClone(ownership.proof),
    exact_rlog_cohort_match: exactCohortMatch,
    exact_sourced_status_event_count_match: exactEventCountMatch,
    input_rlogs: ownershipRlogs instanceof Map ? ownershipRlogs.size : 0,
    selected_status_events: Number(ownership.selected_status_events),
    direct_player_status_events: Number(ownership.direct_player_status_events),
    player_owned_status_events: Number(ownership.player_owned_status_events),
    same_wire_packet_player_owned_status_events:
      Number(ownership.same_wire_packet_player_owned_status_events),
    prior_status_instance_player_owned_status_events:
      Number(ownership.prior_status_instance_player_owned_status_events),
    unique_source_entity_uuids: structuredClone(ownership.unique_source_entity_uuids),
    stable_player_character_id_events: Number(ownership.stable_player_character_id_events),
    stable_player_character_ids: structuredClone(ownership.stable_player_character_ids),
    stable_player_character_id_proven_for_every_status_event:
      ownership.stable_player_character_id_proven_for_every_status_event === true &&
      exactCohortMatch && exactEventCountMatch && statusEventsWithoutSource === 0,
    provider_ownership_proven_for_every_sourced_status_event:
      providerOwnershipProvenForEverySourcedStatusEvent,
    provider_ownership_proven_for_every_status_event:
      providerOwnershipProvenForEveryStatusEvent,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function summarizePartyEffectSeasonContext(
  context,
  entry,
  partyRlogInputs,
  seasonStateMutationReceipt,
) {
  if (!context) return null;
  const contextRlogs = context.input_rlogs;
  const exactCohortMatch = contextRlogs instanceof Map &&
    contextRlogs.size === partyRlogInputs.size &&
    [...partyRlogInputs.entries()].every(([label, expected]) => {
      const actual = contextRlogs.get(label);
      return Number(actual?.bytes) === expected.bytes &&
        String(actual?.sha256 ?? "").toLowerCase() === expected.sha256;
    });
  const exactEventCountMatch =
    Number(context.selected_status_events) === Number(entry?.status_events);
  const everyEventProven = exactCohortMatch && exactEventCountMatch &&
    context.every_selected_event_has_prior_season_context === true;
  const everyEventHasContinuousMonitorCandidate = exactCohortMatch && exactEventCountMatch &&
    context.every_selected_event_has_prior_continuous_monitor_context_candidate === true;
  const everyContinuousMonitorCandidateHasClassifiedGapRoutes =
    everyEventHasContinuousMonitorCandidate &&
    context.every_continuous_monitor_candidate_has_classified_gap_routes === true;
  return {
    proof: structuredClone(context.proof),
    exact_rlog_cohort_match: exactCohortMatch,
    exact_status_event_count_match: exactEventCountMatch,
    input_rlogs: contextRlogs instanceof Map ? contextRlogs.size : 0,
    selected_status_events: Number(context.selected_status_events),
    events_with_prior_season_context: Number(context.events_with_prior_season_context),
    events_with_only_later_season_observation:
      Number(context.events_with_only_later_season_observation),
    events_without_any_season_observation_in_rlog:
      Number(context.events_without_any_season_observation_in_rlog),
    prior_season_ids: structuredClone(context.prior_season_ids),
    prior_continuous_monitor_context_candidates:
      Number(context.prior_continuous_monitor_context_candidates ?? 0),
    prior_continuous_monitor_season_ids:
      structuredClone(context.prior_continuous_monitor_season_ids ?? []),
    every_status_event_has_prior_continuous_monitor_context_candidate:
      everyEventHasContinuousMonitorCandidate,
    continuous_monitor_gap_routes_classified_candidates:
      Number(context.continuous_monitor_gap_routes_classified_candidates ?? 0),
    gap_free_season_source_wire_lane_candidates:
      Number(context.gap_free_season_source_wire_lane_candidates ?? 0),
    no_transport_gap_kind_candidates:
      Number(context.no_transport_gap_kind_candidates ?? 0),
    continuous_monitor_gap_kind_totals:
      structuredClone(context.continuous_monitor_gap_kind_totals ?? {}),
    every_continuous_monitor_candidate_has_classified_gap_routes:
      everyContinuousMonitorCandidateHasClassifiedGapRoutes,
    season_state_mutation_surface: seasonStateMutationReceipt
      ? structuredClone(seasonStateMutationReceipt)
      : null,
    continuous_monitor_context_requires_protocol_event_coverage: true,
    event_time_season_context_proven_for_every_status_event: everyEventProven,
    future_profile_backfill_allowed: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function normalizePartyObservedOriginEdges(entry, effectId, auditSchemaVersion, buildId) {
  if (auditSchemaVersion < 7) return [];
  const edges = entry?.observed_origin_edges;
  const rawPairs = entry?.unique_origin_pairs;
  if (!Array.isArray(edges) || !Array.isArray(rawPairs)) {
    throw new Error(`Party-effect origin edge evidence is missing for ${String(effectId)}`);
  }
  const normalized = [];
  const seen = new Set();
  let observationCount = 0;
  for (const edge of edges) {
    const sourceTypeId = Number(edge?.source_type_id);
    const sourceConfigId = Number(edge?.source_config_id);
    const childEffectId = Number(edge?.child_effect_id);
    const count = Number(edge?.observation_count);
    const key = `${sourceTypeId}:${sourceConfigId}`;
    const exactIdentity = String(buildId) === "24687926"
      ? FIGHT_SOURCE_ENUM_24687926.get(sourceTypeId) ?? null
      : null;
    if (!Number.isSafeInteger(sourceTypeId) ||
      !Number.isSafeInteger(sourceConfigId) || sourceConfigId < 0 ||
      !Number.isSafeInteger(childEffectId) || childEffectId !== effectId ||
      !Number.isSafeInteger(count) || count <= 0 || seen.has(key) ||
      (exactIdentity !== null &&
        (edge?.exact_current_build_enum_identity !== true ||
          edge?.source_kind !== exactIdentity[0] || edge?.source_enum_name !== exactIdentity[1])) ||
      (exactIdentity === null &&
        (edge?.exact_current_build_enum_identity !== false ||
          edge?.source_kind !== "unresolved" || edge?.source_enum_name !== null))) {
      throw new Error(`Unsafe party-effect origin edge ${String(effectId)}:${key}`);
    }
    seen.add(key);
    observationCount += count;
    normalized.push({
      source_type_id: sourceTypeId,
      source_kind: edge.source_kind,
      source_enum_name: edge.source_enum_name,
      source_config_id: sourceConfigId,
      child_effect_id: childEffectId,
      observation_count: count,
      exact_current_build_enum_identity: edge.exact_current_build_enum_identity,
    });
  }
  const pairKeys = rawPairs.map((pair) => {
    if (!Array.isArray(pair) || pair.length !== 2 ||
      !Number.isSafeInteger(Number(pair[0])) ||
      !Number.isSafeInteger(Number(pair[1])) || Number(pair[1]) < 0) {
      throw new Error(`Unsafe party-effect origin pair for ${String(effectId)}`);
    }
    return `${Number(pair[0])}:${Number(pair[1])}`;
  });
  if (new Set(pairKeys).size !== pairKeys.length ||
    stableStringify([...seen].sort()) !== stableStringify([...pairKeys].sort()) ||
    observationCount > Number(entry?.status_events)) {
    throw new Error(`Party-effect origin edge counts disagree for ${String(effectId)}`);
  }
  return normalized.sort((left, right) =>
    left.source_type_id - right.source_type_id || left.source_config_id - right.source_config_id
  );
}

function emptyPartyDamageActionEdgeSummary() {
  return {
    edge_count: 0,
    effect_target_as_damage_actor_edges: 0,
    effect_target_as_damage_target_edges: 0,
    effect_target_as_damage_actor_event_references: 0,
    effect_target_as_damage_target_event_references: 0,
  };
}

function validatePartyDamageActionWindows(windows, document) {
  const byEffect = new Map();
  let edgeCount = 0;
  let actorEdgeCount = 0;
  let targetEdgeCount = 0;
  for (const window of windows) {
    const effectId = Number(window?.effect_id);
    const effectTargetActorId = String(window?.effect_target_actor_id ?? "");
    const effectTargetEntityUuid = String(window?.effect_target_entity_uuid ?? "");
    const edges = window?.damage_action_edges;
    if (!Number.isSafeInteger(effectId) || effectId <= 0 ||
      !/^\d+$/.test(effectTargetActorId) || !/^-?\d+$/.test(effectTargetEntityUuid) ||
      effectTargetActorId !== String(window?.affected_entity_actor_id ?? "") ||
      effectTargetEntityUuid !== String(window?.affected_entity_uuid ?? "") ||
      !Array.isArray(edges) ||
      !isValidPartyDamageRelation(window?.affected_entity_damage_actions) ||
      !isValidPartyDamageRelation(window?.damage_actions_targeting_affected_entity)) {
      throw new Error(`Unsafe party-effect damage-action window for ${String(window?.effect_id)}`);
    }
    const summary = byEffect.get(effectId) ?? emptyPartyDamageActionEdgeSummary();
    let actorEvents = 0;
    let targetEvents = 0;
    let actorAmount = 0n;
    let targetAmount = 0n;
    const uniqueEdges = new Set();
    for (const edge of edges) {
      const role = edge?.role;
      const sourceActorId = String(edge?.damage_source_actor_id ?? "");
      const sourceEntityUuid = String(edge?.damage_source_entity_uuid ?? "");
      const directSourceActorId = edge?.direct_damage_source_actor_id;
      const directSourceEntityUuid = edge?.direct_damage_source_entity_uuid;
      const targetActorId = String(edge?.damage_target_actor_id ?? "");
      const targetEntityUuid = String(edge?.damage_target_entity_uuid ?? "");
      const abilityId = edge?.ability_id;
      const eventCount = Number(edge?.event_count);
      const firstSequence = Number(edge?.first_sequence);
      const lastSequence = Number(edge?.last_sequence);
      const firstObservedMicros = Number(edge?.first_observed_micros);
      const lastObservedMicros = Number(edge?.last_observed_micros);
      const samples = edge?.samples;
      const directPairValid = (directSourceActorId === null && directSourceEntityUuid === null) ||
        (/^\d+$/.test(String(directSourceActorId ?? "")) &&
          /^-?\d+$/.test(String(directSourceEntityUuid ?? "")));
      const abilityValid = abilityId === null ||
        (Number.isSafeInteger(Number(abilityId)) && Number(abilityId) >= 0);
      const amountText = String(edge?.amount ?? "");
      if (!new Set([
        "effect_target_is_damage_actor",
        "effect_target_is_damage_target",
      ]).has(role) || !/^\d+$/.test(sourceActorId) || !/^-?\d+$/.test(sourceEntityUuid) ||
        !directPairValid || !abilityValid || !/^\d+$/.test(targetActorId) ||
        !/^-?\d+$/.test(targetEntityUuid) || !/^\d+$/.test(amountText) ||
        !Number.isSafeInteger(eventCount) || eventCount <= 0 ||
        !Number.isSafeInteger(firstSequence) || firstSequence < 0 ||
        !Number.isSafeInteger(lastSequence) || lastSequence < firstSequence ||
        !Number.isSafeInteger(firstObservedMicros) || firstObservedMicros < 0 ||
        !Number.isSafeInteger(lastObservedMicros) || lastObservedMicros < firstObservedMicros ||
        !Array.isArray(samples) || samples.length === 0 || samples.length > 4 ||
        samples.length > eventCount || edge?.causal_attribution_authorized !== false ||
        edge?.provider_rdps_credit_authorized !== false ||
        (role === "effect_target_is_damage_actor" && sourceActorId !== effectTargetActorId) ||
        (role === "effect_target_is_damage_target" && targetActorId !== effectTargetActorId)) {
        throw new Error(`Unsafe party-effect damage-action edge for ${effectId}`);
      }
      if (samples.some((sample) => {
        const sequence = Number(sample?.sequence);
        const observedMicros = Number(sample?.observed_micros);
        return !Number.isSafeInteger(sequence) || sequence < firstSequence || sequence > lastSequence ||
          !Number.isSafeInteger(observedMicros) || observedMicros < firstObservedMicros ||
          observedMicros > lastObservedMicros ||
          !Number.isSafeInteger(Number(sample?.amount)) ||
          (sample?.actual_amount !== null &&
            !Number.isSafeInteger(Number(sample?.actual_amount))) ||
          (sample?.hit_event_id !== null &&
            !Number.isSafeInteger(Number(sample?.hit_event_id))) ||
          (sample?.skill_effect_uuid !== null &&
            !/^-?\d+$/.test(String(sample?.skill_effect_uuid ?? "")));
      })) {
        throw new Error(`Unsafe party-effect damage-action sample for ${effectId}`);
      }
      const edgeKey = [
        role, sourceActorId, sourceEntityUuid,
        directSourceActorId === null ? "" : String(directSourceActorId),
        directSourceEntityUuid === null ? "" : String(directSourceEntityUuid),
        abilityId === null ? "" : String(abilityId), targetActorId, targetEntityUuid,
      ].join(":");
      if (uniqueEdges.has(edgeKey)) {
        throw new Error(`Duplicate party-effect damage-action edge for ${effectId}`);
      }
      uniqueEdges.add(edgeKey);
      const edgeAmount = BigInt(amountText);
      edgeCount += 1;
      summary.edge_count += 1;
      if (role === "effect_target_is_damage_actor") {
        actorEdgeCount += 1;
        actorEvents += eventCount;
        actorAmount += edgeAmount;
        summary.effect_target_as_damage_actor_edges += 1;
        summary.effect_target_as_damage_actor_event_references += eventCount;
      } else {
        targetEdgeCount += 1;
        targetEvents += eventCount;
        targetAmount += edgeAmount;
        summary.effect_target_as_damage_target_edges += 1;
        summary.effect_target_as_damage_target_event_references += eventCount;
      }
    }
    if (actorEvents !== Number(window.affected_entity_damage_actions.event_count) ||
      targetEvents !== Number(window.damage_actions_targeting_affected_entity.event_count) ||
      actorAmount !== BigInt(String(window.affected_entity_damage_actions.amount)) ||
      targetAmount !== BigInt(String(window.damage_actions_targeting_affected_entity.amount))) {
      throw new Error(`Party-effect damage-action edges do not conserve window relations for ${effectId}`);
    }
    byEffect.set(effectId, summary);
  }
  if (edgeCount !== Number(document?.summary?.window_damage_action_edges) ||
    actorEdgeCount !== Number(document?.summary?.window_damage_action_actor_edges) ||
    targetEdgeCount !== Number(document?.summary?.window_damage_action_target_edges) ||
    edgeCount !== actorEdgeCount + targetEdgeCount) {
    throw new Error("Party-effect damage-action edge summary is inconsistent");
  }
  return { byEffect, edgeCount, actorEdgeCount, targetEdgeCount };
}

function isValidPartyDamageRelation(relation) {
  return Number.isSafeInteger(Number(relation?.event_count)) && Number(relation.event_count) >= 0 &&
    /^\d+$/.test(String(relation?.amount ?? "")) &&
    Array.isArray(relation?.ability_ids) &&
    Array.isArray(relation?.damage_source_actor_ids) &&
    Array.isArray(relation?.damage_target_actor_ids);
}

function isValidPartyEffectProviderOwnershipJoin(entry, ownershipProofInputs) {
  const receipt = entry?.provider_ownership;
  const observed = Number(entry?.status_events) > 0;
  const ownershipBlocker = "provider-ownership-for-every-status-event-open";
  if (receipt === null) {
    return entry?.provider_ownership_proven_for_every_sourced_status_event === false &&
      entry?.provider_ownership_proven_for_every_status_event === false &&
      (!observed || entry?.blockers?.includes(ownershipBlocker));
  }
  if (!receipt || !isValidFileDescriptor(receipt.proof) ||
    !ownershipProofInputs.some((input) =>
      stableStringify(input) === stableStringify(receipt.proof)) ||
    !Number.isSafeInteger(Number(receipt.input_rlogs)) || Number(receipt.input_rlogs) <= 0 ||
    !Number.isSafeInteger(Number(receipt.selected_status_events)) ||
    Number(receipt.selected_status_events) <= 0 ||
    !Number.isSafeInteger(Number(receipt.direct_player_status_events)) ||
    !Number.isSafeInteger(Number(receipt.player_owned_status_events)) ||
    !Number.isSafeInteger(Number(receipt.same_wire_packet_player_owned_status_events)) ||
    !Number.isSafeInteger(Number(receipt.prior_status_instance_player_owned_status_events)) ||
    !Number.isSafeInteger(Number(receipt.stable_player_character_id_events)) ||
    !Array.isArray(receipt.unique_source_entity_uuids) ||
    !Array.isArray(receipt.stable_player_character_ids) ||
    receipt.formula_authority !== false || receipt.runtime_authority !== false ||
    receipt.ui_display_authority !== false || receipt.provider_rdps_credit_allowed !== false) {
    return false;
  }
  const playerOwnedEvents = Number(receipt.direct_player_status_events) +
    Number(receipt.player_owned_status_events) +
    Number(receipt.same_wire_packet_player_owned_status_events) +
    Number(receipt.prior_status_instance_player_owned_status_events);
  const expectedSourcedProof = receipt.exact_rlog_cohort_match === true &&
    receipt.exact_sourced_status_event_count_match === true &&
    playerOwnedEvents === Number(receipt.selected_status_events);
  const expectedAllEventProof = expectedSourcedProof &&
    Number(entry?.status_events_without_source) === 0;
  return entry?.provider_ownership_proven_for_every_sourced_status_event ===
      expectedSourcedProof &&
    receipt.provider_ownership_proven_for_every_sourced_status_event ===
      expectedSourcedProof &&
    entry?.provider_ownership_proven_for_every_status_event === expectedAllEventProof &&
    receipt.provider_ownership_proven_for_every_status_event === expectedAllEventProof &&
    (expectedAllEventProof
      ? !entry?.blockers?.includes(ownershipBlocker)
      : (!observed || entry?.blockers?.includes(ownershipBlocker)));
}

function isValidPartyEffectSeasonContextJoin(
  entry,
  seasonContextProofInputs,
  seasonStateMutationProofInput,
  closureSchemaVersion,
) {
  const receipt = entry?.status_event_season_context;
  const genericBlocker = "event-time-season-context-for-status-events-open";
  const continuousBlocker =
    "continuous-monitor-season-context-awaiting-protocol-event-coverage";
  const classifiedGapRouteBlocker =
    "continuous-monitor-season-context-gap-routes-classified-awaiting-season-state-mutation-and-promoted-protocol-coverage";
  const directWriterBlocker =
    "continuous-monitor-season-context-direct-writers-proven-awaiting-intervening-clear-exclusion-and-promoted-protocol-coverage";
  const reconnectLifecycleBlocker =
    "continuous-monitor-season-context-normal-reconnect-preserves-state-awaiting-explicit-logout-exclusion-and-promoted-protocol-coverage";
  const requiresContext = entry?.exact_effect_to_stat_coefficient_proven === true;
  if (receipt === null) {
    return entry?.event_time_season_context_proven_for_every_status_event === false &&
      (!requiresContext || entry?.blockers?.includes(genericBlocker));
  }
  if (!receipt || !isValidFileDescriptor(receipt.proof) ||
    !seasonContextProofInputs.some((input) =>
      stableStringify(input) === stableStringify(receipt.proof)) ||
    !Number.isSafeInteger(Number(receipt.input_rlogs)) || Number(receipt.input_rlogs) <= 0 ||
    !Number.isSafeInteger(Number(receipt.selected_status_events)) ||
    !Number.isSafeInteger(Number(receipt.events_with_prior_season_context)) ||
    !Number.isSafeInteger(Number(receipt.events_with_only_later_season_observation)) ||
    !Number.isSafeInteger(Number(receipt.events_without_any_season_observation_in_rlog)) ||
    Number(receipt.events_with_prior_season_context) +
      Number(receipt.events_with_only_later_season_observation) +
      Number(receipt.events_without_any_season_observation_in_rlog) !==
        Number(receipt.selected_status_events) ||
    !Array.isArray(receipt.prior_season_ids) ||
    receipt.prior_season_ids.some((value) => !Number.isSafeInteger(value) || value <= 0) ||
    !Number.isSafeInteger(Number(receipt.prior_continuous_monitor_context_candidates)) ||
    Number(receipt.prior_continuous_monitor_context_candidates) < 0 ||
    Number(receipt.prior_continuous_monitor_context_candidates) >
      Number(receipt.selected_status_events) ||
    !Array.isArray(receipt.prior_continuous_monitor_season_ids) ||
    receipt.prior_continuous_monitor_season_ids.some((value) =>
      !Number.isSafeInteger(value) || value <= 0
    ) || receipt.continuous_monitor_context_requires_protocol_event_coverage !== true ||
    receipt.future_profile_backfill_allowed !== false ||
    receipt.formula_authority !== false || receipt.runtime_authority !== false ||
    receipt.ui_display_authority !== false || receipt.provider_rdps_credit_allowed !== false ||
    (closureSchemaVersion >= 40 &&
      (!Number.isSafeInteger(Number(receipt.continuous_monitor_gap_routes_classified_candidates)) ||
        Number(receipt.continuous_monitor_gap_routes_classified_candidates) < 0 ||
        Number(receipt.continuous_monitor_gap_routes_classified_candidates) >
          Number(receipt.prior_continuous_monitor_context_candidates) ||
        !Number.isSafeInteger(Number(receipt.gap_free_season_source_wire_lane_candidates)) ||
        Number(receipt.gap_free_season_source_wire_lane_candidates) < 0 ||
        Number(receipt.gap_free_season_source_wire_lane_candidates) >
          Number(receipt.continuous_monitor_gap_routes_classified_candidates) ||
        !Number.isSafeInteger(Number(receipt.no_transport_gap_kind_candidates)) ||
        Number(receipt.no_transport_gap_kind_candidates) < 0 ||
        Number(receipt.no_transport_gap_kind_candidates) >
          Number(receipt.continuous_monitor_gap_routes_classified_candidates) ||
        !receipt.continuous_monitor_gap_kind_totals ||
        ["capture_drop", "tcp_gap", "unknown_route", "decode_failure", "unsupported_fragment"]
          .some((name) =>
            !Number.isSafeInteger(Number(receipt.continuous_monitor_gap_kind_totals[name])) ||
            Number(receipt.continuous_monitor_gap_kind_totals[name]) < 0
          ))) ||
    (closureSchemaVersion >= 41 &&
      (!receipt.season_state_mutation_surface ||
        !isValidFileDescriptor(receipt.season_state_mutation_surface.proof) ||
        stableStringify(receipt.season_state_mutation_surface.proof) !==
          stableStringify(seasonStateMutationProofInput) ||
        receipt.season_state_mutation_surface
          .exact_server_route_to_positive_season_writer_proven !== true ||
        receipt.season_state_mutation_surface.exact_clear_to_zero_writer_proven !== true ||
        receipt.season_state_mutation_surface.direct_literal_lua_writer_surface_complete !== true ||
        receipt.season_state_mutation_surface.dynamic_or_aliased_writers_proven_absent !== false ||
        receipt.season_state_mutation_surface
          .intervening_monitor_chain_clear_proven_absent !== false ||
        receipt.season_state_mutation_surface.promoted_protocol_event_coverage_proven !== false ||
        receipt.season_state_mutation_surface.event_time_season_authority !== false ||
        receipt.season_state_mutation_surface.formula_authority !== false ||
        receipt.season_state_mutation_surface.runtime_authority !== false ||
        receipt.season_state_mutation_surface.ui_display_authority !== false ||
        receipt.season_state_mutation_surface.provider_rdps_credit_allowed !== false))) {
    return false;
  }
  if (closureSchemaVersion >= 42 && receipt !== null &&
    (!receipt.season_state_mutation_surface ||
      Number(receipt.season_state_mutation_surface.proof_schema_version) !== 2 ||
      receipt.season_state_mutation_surface
        .normal_reconnect_preserves_season_state_by_static_control_flow !== true ||
      receipt.season_state_mutation_surface
        .explicit_logout_resets_season_state_by_static_control_flow !== true ||
      Number(receipt.season_state_mutation_surface.direct_data_manager_clear_callsites) !== 1 ||
      receipt.season_state_mutation_surface
        .intervening_monitor_chain_explicit_logout_proven_absent !== false)) {
    return false;
  }
  const expectedProof = receipt.exact_rlog_cohort_match === true &&
    receipt.exact_status_event_count_match === true &&
    Number(receipt.events_with_prior_season_context) === Number(receipt.selected_status_events);
  const everyContinuousCandidate = receipt.exact_rlog_cohort_match === true &&
    receipt.exact_status_event_count_match === true &&
    Number(receipt.prior_continuous_monitor_context_candidates) ===
      Number(receipt.selected_status_events);
  const everyCandidateHasClassifiedGapRoutes = closureSchemaVersion >= 40 &&
    everyContinuousCandidate &&
    Number(receipt.continuous_monitor_gap_routes_classified_candidates) ===
      Number(receipt.prior_continuous_monitor_context_candidates);
  const directWriterSurfaceProven = closureSchemaVersion >= 41 &&
    everyCandidateHasClassifiedGapRoutes &&
    receipt.season_state_mutation_surface?.direct_literal_lua_writer_surface_complete === true;
  const reconnectLifecycleProven = closureSchemaVersion >= 42 &&
    directWriterSurfaceProven &&
    receipt.season_state_mutation_surface
      ?.normal_reconnect_preserves_season_state_by_static_control_flow === true;
  const expectedBlocker = reconnectLifecycleProven
    ? reconnectLifecycleBlocker
    : directWriterSurfaceProven
    ? directWriterBlocker
    : everyCandidateHasClassifiedGapRoutes
    ? classifiedGapRouteBlocker
    : everyContinuousCandidate ? continuousBlocker : genericBlocker;
  return entry?.event_time_season_context_proven_for_every_status_event === expectedProof &&
    receipt.event_time_season_context_proven_for_every_status_event === expectedProof &&
    receipt.every_status_event_has_prior_continuous_monitor_context_candidate ===
      everyContinuousCandidate &&
    (closureSchemaVersion < 40 ||
      receipt.every_continuous_monitor_candidate_has_classified_gap_routes ===
        everyCandidateHasClassifiedGapRoutes) &&
    (!requiresContext || (expectedProof
      ? !entry?.blockers?.includes(genericBlocker) &&
        !entry?.blockers?.includes(continuousBlocker) &&
        !entry?.blockers?.includes(classifiedGapRouteBlocker) &&
        !entry?.blockers?.includes(directWriterBlocker) &&
        !entry?.blockers?.includes(reconnectLifecycleBlocker)
      : entry?.blockers?.includes(expectedBlocker)));
}

function isValidPartyHasteStackingJoin(
  entry,
  stackingProofInput,
  actionFormulaProofInput,
  closureSchemaVersion,
) {
  if (closureSchemaVersion < 43) return true;
  const receipt = entry?.stacking_frontier;
  const oldBlocker = "stacking-arbitration-operation-order-and-downstream-rounding-open";
  const refinedBlocker =
    "stacking-arbitration-unobserved-static-integer-semantics-and-downstream-operation-order-rounding-open";
  const actionProvenBlocker = "stacking-arbitration-unobserved-static-integer-semantics-open";
  if (Number(entry?.effect_id) !== 31602) return receipt === null;
  if (stackingProofInput === null) {
    return receipt === null && entry?.blockers?.includes(oldBlocker) &&
      !entry?.blockers?.includes(refinedBlocker);
  }
  return receipt !== null && isValidFileDescriptor(receipt?.proof) &&
    stableStringify(receipt.proof) === stableStringify(stackingProofInput) &&
    receipt.exact_party_effect_window_audit_match === true &&
    stableStringify(receipt.exact_static_repeat_add_rule) === stableStringify([1, 1]) &&
    Number(receipt.exact_static_time_refresh_type) === 0 &&
    Number(receipt.selected_status_events) === Number(entry?.status_events) &&
    Number(receipt.selected_status_events) === 130 && Number(receipt.selected_windows) === 65 &&
    stableStringify(receipt.reported_stack_values) === stableStringify([1]) &&
    Number(receipt.overlapping_window_pairs) === 0 &&
    Number(receipt.distinct_provider_overlap_pairs) === 0 &&
    Number(receipt.max_concurrent_windows_for_same_session_and_target) === 1 &&
    receipt.exact_static_integer_rule_semantics_proven === false &&
    receipt.server_stacking_arbitration_proven === false &&
    receipt.downstream_operation_order_and_rounding_proven === false &&
    receipt.formula_authority === false && receipt.runtime_authority === false &&
    receipt.ui_display_authority === false && receipt.provider_rdps_credit_allowed === false &&
    (actionFormulaProofInput === null
      ? entry?.blockers?.includes(refinedBlocker) && !entry?.blockers?.includes(actionProvenBlocker)
      : entry?.blockers?.includes(actionProvenBlocker) && !entry?.blockers?.includes(refinedBlocker)) &&
    !entry?.blockers?.includes(oldBlocker);
}

function isValidPartyHasteActionJoin(
  entry,
  formulaProofInput,
  capacityProofInput,
  ancestryProofInput,
  closureSchemaVersion,
) {
  if (closureSchemaVersion < 44) return true;
  const receipt = entry?.action_speed_counterfactual_frontier;
  const inputs = [formulaProofInput, capacityProofInput, ancestryProofInput];
  const supplied = inputs.filter((value) => value !== null).length;
  const blocker =
    "native-action-speed-operation-order-proven-awaiting-bit-equivalent-action-time-input-and-temporary-term-replay-action-opportunity-packet-clock-and-conservation";
  if (Number(entry?.effect_id) !== 31602) return receipt === null;
  if (supplied === 0) return receipt === null && !entry?.blockers?.includes(blocker);
  if (supplied !== 3 || receipt === null) return false;
  return isValidFileDescriptor(receipt.native_action_speed_formula_proof) &&
    stableStringify(receipt.native_action_speed_formula_proof) ===
      stableStringify(formulaProofInput) &&
    isValidFileDescriptor(receipt.conditional_capacity_proof) &&
    stableStringify(receipt.conditional_capacity_proof) === stableStringify(capacityProofInput) &&
    isValidFileDescriptor(receipt.action_timing_ancestry_proof) &&
    stableStringify(receipt.action_timing_ancestry_proof) === stableStringify(ancestryProofInput) &&
    receipt.exact_native_float32_operation_order_proven === true &&
    receipt.exact_non_singing_algebraic_speed_formulas_proven === true &&
    receipt.exact_temporary_attribute_lookup_abi_proven === true &&
    receipt.exact_action_speed_native_sampling_point_proven === true &&
    receipt.exact_scheduler_speed_scaling_formula_proven === true &&
    Number(receipt.responsive_damage_action_memberships) === 3713 &&
    String(receipt.responsive_reported_damage_units) === "556129992" &&
    Number(receipt.conditional_capacity_groups) === 54 &&
    Number(receipt.conditional_capacity_memberships) === 3185 &&
    String(receipt.conditional_capacity_reported_damage_units) === "350049695" &&
    Number(receipt.self_provider_exclusion_memberships) === 528 &&
    Number(receipt.exact_one_damage_event_match_memberships) === 3466 &&
    Number(receipt.unresolved_no_damage_event_memberships) === 247 &&
    receipt.conditional_capacity_formula ===
      "reported_damage * provider_coefficient / (10000 + observed_attribute)" &&
    Number(receipt.fixed_point_scale) === 10000 &&
    receipt.bit_equivalent_float32_input_snapshot_replay_proven === false &&
    receipt.exact_action_opportunity_proven === false &&
    receipt.packet_clock_correspondence_proven === false &&
    receipt.integer_damage_rounding_proven === false &&
    receipt.packet_conservation_proven === false &&
    Number(receipt.observed_damage_reassigned_to_provider) === 0 &&
    receipt.formula_authority === false && receipt.runtime_authority === false &&
    receipt.ui_display_authority === false && receipt.provider_rdps_credit_allowed === false &&
    entry?.blockers?.includes(blocker);
}

function imagineFormulaBlocker(effectId, receipt) {
  if (Number(effectId) === 2110124 && receipt?.nontransfer_routing_marker_proven === true) {
    return "provider-side-routing-marker-proven-nontransfer-zero-provider-credit-by-design";
  }
  if (Number(effectId) === 2110125) {
    if (receipt?.damage_stage_frontier) {
      return "exact-event-time-tier-source-lifecycle-and-27238-gap-bounded-damage-memberships-proven-controlled-pair-exhausted-awaiting-damage-consumer-operation-order-rounding-stacking-projection-and-conservation";
    }
    return "exact-tier-formula-and-packet-oracle-proven-awaiting-event-time-provider-tier-damage-stage-rounding-and-conservation";
  }
  if (Number(effectId) === 2110140) {
    if (receipt?.tier_window_counterfactual_inputs) {
      return "exact-tier-scalar-primary-transform-runtime-input-route-and-8-occurrence-scoped-provider-tiers-recipient-snapshots-and-12557-damage-actions-retained-awaiting-128-tiers-primary-raw-percent-evaluation-base-attack-update-order-damage-stage-rounding-and-conservation";
    }
    if (receipt?.status_attribute_tier_evidence) {
      return "exact-tier-scalar-primary-transform-runtime-input-route-and-8-occurrence-scoped-provider-tiers-proven-awaiting-128-tiers-recipient-snapshot-damage-stage-and-conservation";
    }
    return "exact-tier-scalar-primary-transform-and-runtime-input-route-proven-awaiting-event-time-provider-tier-recipient-snapshot-damage-stage-and-conservation";
  }
  if (Number(effectId) === 2110034) {
    return "exact-cooldown-scalar-and-native-equation-proven-awaiting-category-map-cast-schedule-and-conservation";
  }
  return "imagine-formula-receipt-present-downstream-replay-open";
}

function isValidImagineFormulaJoin(
  entry,
  imagineProofInput,
  imagineStatusAttributeTierProofInput,
  imagineTierWindowCounterfactualInputsInput,
  fatalSpiralDamageStageFrontierInput,
  closureSchemaVersion,
) {
  if (closureSchemaVersion < 45) return true;
  const effectId = Number(entry?.effect_id);
  const receipt = entry?.imagine_formula;
  const knownEffects = new Set([2110034, 2110124, 2110125, 2110140]);
  if (imagineProofInput === null) return receipt === null;
  if (!knownEffects.has(effectId)) return receipt === null;
  if (!receipt || !isValidFileDescriptor(receipt.proof) ||
    stableStringify(receipt.proof) !== stableStringify(imagineProofInput) ||
    Number(receipt.observed_damage_reassigned_to_provider) !== 0 ||
    receipt.formula_authority !== false || receipt.runtime_authority !== false ||
    receipt.ui_display_authority !== false || receipt.provider_rdps_credit_allowed !== false ||
    !entry?.blockers?.includes(imagineFormulaBlocker(effectId, receipt)) ||
    entry?.blockers?.includes("exact-effect-magnitude-operation-order-stacking-and-rounding-open")) {
    return false;
  }
  if (effectId === 2110124) {
    return Number(receipt.imagine_skill_id) === 3957 &&
      receipt.component_id === "fatal-spiral-caster-side-marker" &&
      receipt.nontransfer_routing_marker_proven === true &&
      receipt.disposition === "provider-side-routing-marker-never-count-or-transfer-as-damage" &&
      !entry?.blockers?.includes("counterfactual-damage-projection-and-conservation-open");
  }
  if (effectId === 2110125) {
    const damageStage = receipt.damage_stage_frontier ?? null;
    const damageStageValid = closureSchemaVersion < 53 ||
      (fatalSpiralDamageStageFrontierInput === null
        ? damageStage === null
        : damageStage !== null &&
          stableStringify(damageStage.proof) ===
            stableStringify(fatalSpiralDamageStageFrontierInput) &&
          damageStage.exact_source_side_effect_recipient_to_damage_actor_join_complete === true &&
          Number(damageStage.exact_gap_bounded_lifecycles) === 29 &&
           Number(damageStage.audited_damage_event_memberships) === 27238 &&
          (closureSchemaVersion < 69 ||
            (damageStage.exact_gap_safe_lifecycle_action_membership_join_complete === true &&
              damageStage.provider_ownership_proven_for_all_gap_safe_memberships === true &&
              isValidFileDescriptor(damageStage.gap_safe_lifecycle_action_summary) &&
              Number(damageStage.gap_safe_damage_memberships) === 27238 &&
              Number(damageStage.gap_safe_third_party_provider_memberships) === 25679 &&
              Number(damageStage.gap_safe_provider_self_memberships) === 1559 &&
              Number(damageStage.gap_safe_ownership_unresolved_memberships) === 0 &&
              String(damageStage.gap_safe_reported_damage_membership_sum) === "3501998794" &&
              stableStringify(damageStage.gap_safe_source_protocol_pack_digests) ===
                stableStringify([
                  "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
                ]))) &&
           Number(damageStage.retained_formula_samples) === 27001 &&
          stableStringify(damageStage.observed_generic_element_attribute_values) ===
            stableStringify([316, 1316]) &&
          Number(damageStage.controlled_counterfactual_pairs) === 0 &&
           (closureSchemaVersion < 56 ||
             (damageStage.automatic_integer_candidate_evaluator_integrated === true &&
               damageStage.automatic_integer_candidate_model_id ===
                 "effect-2110125-source-all-element-current-final-multiplier-candidate" &&
               Number(damageStage.automatic_integer_candidate_analyzer_schema_version) === 17 &&
               Number(damageStage.automatic_integer_candidate_evaluated_variants) > 0)) &&
          (closureSchemaVersion < 57 ||
            (damageStage
              .retained_current_build_present_and_absent_capture_frontier_exhausted === true &&
              Number(damageStage.reviewed_current_build_rlogs) === 26 &&
              Number(damageStage.observed_damage_action_ids) === 92 &&
              Number(damageStage.all_reviewed_comparison_samples) === 488546 &&
              Number(damageStage.broad_diagnostic_absent_pairs) === 12176)) &&
          (closureSchemaVersion < 58 ||
            (damageStage.exact_build_hidden_controlled_capture_surface_identified === true &&
              damageStage.shipping_client_blocks_controlled_capture_submission === true &&
              damageStage.ordinary_production_account_controlled_capture_authorized === false &&
              damageStage.controlled_capture_currently_executable === false)) &&
          (closureSchemaVersion < 59 ||
            (damageStage.exact_build_training_scene_access_frontier_reviewed === true &&
              damageStage.ordinary_training_scene_entry_route_proven === false &&
              damageStage.training_scene_controlled_capture_currently_executable === false)) &&
          (closureSchemaVersion < 60 ||
            (damageStage.exact_native_immediate_family_search_exhausted === true &&
              damageStage.combat_relevant_exact_family_immediate_consumer_found === false &&
              damageStage.computed_indirect_table_driven_or_protected_consumer_excluded === false)) &&
          (closureSchemaVersion < 61 ||
            (damageStage.retained_recovered_partial_prefix_frontier_exhausted === true &&
              damageStage.recovered_partial_prefix_source_capture_integrity_seal_authority === false &&
              Number(damageStage.recovered_partial_prefix_validated_events) === 1039616 &&
              Number(damageStage.recovered_partial_prefix_comparison_samples) === 92161 &&
              Number(damageStage.recovered_partial_prefix_controlled_pairs) === 0)) &&
          damageStage.damage_stage_consumer_proven === false &&
          damageStage.operation_order_proven === false &&
          damageStage.integer_rounding_proven === false &&
          damageStage.conservation_replay_complete === false &&
          damageStage.formula_authority === false &&
          damageStage.runtime_authority === false &&
          damageStage.ui_display_authority === false &&
          damageStage.provider_rdps_credit_allowed === false &&
          Number(damageStage.observed_damage_reassigned_to_provider) === 0);
    return damageStageValid && Number(receipt.imagine_skill_id) === 3957 &&
      receipt.component_id === "fatal-spiral-shared-all-element-bonus" &&
      receipt.exact_component_scalar_available === true &&
      Number(receipt.fixed_point_denominator) === 10000 &&
      receipt.equation === "all_element_bonus_basis_points = 500 + tier_attr_per" &&
      stableStringify(receipt.tier_total_basis_points) ===
        stableStringify([600, 700, 800, 900, 1000]) &&
      Number(receipt.packet_oracle_tier) === 5 &&
      Number(receipt.packet_oracle_applied_delta) === 1000 &&
      Number(receipt.packet_oracle_removed_delta) === -1000 &&
      receipt.provider_tier_snapshot_complete === (damageStage !== null) &&
      receipt.affected_hit_rows_selected === (damageStage !== null) &&
      receipt.integer_damage_counterfactual_complete === false &&
      receipt.conservation_replay_complete === false;
  }
  if (effectId === 2110140) {
    const expectedClassTransforms = [
      [1, 11030, 11330, 11332, 1250, "1/8"],
      [2, 11020, 11340, 11342, 1000, "1/10"],
      [3, 11010, 11330, 11332, 1250, "1/8"],
      [4, 11010, 11330, 11332, 1250, "1/8"],
      [5, 11020, 11340, 11342, 1000, "1/10"],
      [9, 11010, 11330, 11332, 1250, "1/8"],
      [11, 11030, 11330, 11332, 1250, "1/8"],
      [12, 11010, 11330, 11332, 1250, "1/8"],
      [13, 11020, 11340, 11342, 1000, "1/10"],
    ];
    const observedClassTransforms = (receipt.class_primary_attack_transforms ?? []).map(
      (route) => [
        Number(route.class_id),
        Number(route.primary_attribute_id),
        Number(route.attack_attribute_id),
        Number(route.attack_add_attribute_id),
        Number(route.coefficient_basis_points),
        String(route.exact_ratio),
      ],
    );
    const tierEvidence = receipt.status_attribute_tier_evidence ?? null;
    const tierEvidenceValid = closureSchemaVersion < 47 ||
      (imagineStatusAttributeTierProofInput === null
        ? tierEvidence === null
        : tierEvidence !== null &&
          isValidFileDescriptor(tierEvidence.proof) &&
          stableStringify(tierEvidence.proof) ===
            stableStringify(imagineStatusAttributeTierProofInput) &&
          tierEvidence.resolution_scope ===
            "exact-provider-status-instance-recipient-lifecycle-occurrence-only" &&
          Number(tierEvidence.exact_paired_attribute_occurrences) === 8 &&
          Number(tierEvidence.exact_base_tier_occurrences) === 2 &&
          Number(tierEvidence.exact_tier_5_occurrences) === 6 &&
          Number(tierEvidence.unresolved_applied_status_instances) === 128 &&
          tierEvidence.provider_tier_snapshot_complete === false &&
          tierEvidence.formula_authority === false && tierEvidence.runtime_authority === false &&
          tierEvidence.ui_display_authority === false &&
          tierEvidence.provider_rdps_credit_allowed === false &&
          Number(tierEvidence.observed_damage_reassigned_to_provider) === 0);
    const windowInputs = receipt.tier_window_counterfactual_inputs ?? null;
    const windowInputsValid = closureSchemaVersion < 48 ||
      (imagineTierWindowCounterfactualInputsInput === null
        ? windowInputs === null
        : windowInputs !== null &&
          isValidFileDescriptor(windowInputs.proof) &&
          stableStringify(windowInputs.proof) ===
            stableStringify(imagineTierWindowCounterfactualInputsInput) &&
          windowInputs.resolution_scope ===
            "eight-exact-provider-status-instance-recipient-windows-only" &&
          Number(windowInputs.exact_apply_remove_windows) === 8 &&
          Number(windowInputs.complete_window_inputs) === 8 &&
          Number(windowInputs.retained_recipient_damage_actions) === 12557 &&
          String(windowInputs.retained_hp_loss) === "1923279061" &&
          String(windowInputs.retained_reported_damage) === "1947659979" &&
          Number(windowInputs.single_effect_provider_damage_actions) === 12557 &&
          Number(windowInputs.concurrent_effect_provider_damage_actions) === 0 &&
          windowInputs.global_provider_tier_snapshot_complete === false &&
          windowInputs.global_recipient_pre_effect_attribute_snapshot_complete === false &&
          windowInputs.retained_actions_have_counterfactual_damage_delta === false &&
          windowInputs.integer_damage_stage_order_and_rounding_proven === false &&
          windowInputs.formula_authority === false && windowInputs.runtime_authority === false &&
          windowInputs.ui_display_authority === false &&
          windowInputs.provider_rdps_credit_allowed === false &&
          Number(windowInputs.observed_damage_reassigned_to_provider) === 0);
    return tierEvidenceValid && windowInputsValid && Number(receipt.imagine_skill_id) === 3971 &&
      stableStringify(receipt.component_ids) === stableStringify([
        "superconductor-surge-mechanical-power-main-stats",
        "superconductor-surge-mechanical-power-healing-received",
      ]) && receipt.exact_component_scalar_available === true &&
      stableStringify(observedClassTransforms) === stableStringify(expectedClassTransforms) &&
      receipt.provider_tier_snapshot_complete === false &&
      receipt.recipient_pre_effect_attribute_snapshot_complete === false &&
      receipt.recipient_effect_attribute_delta_replay_complete === false &&
      receipt.affected_hit_rows_selected === false &&
      receipt.integer_damage_counterfactual_complete === false &&
      receipt.healing_lane_damage_credit_allowed === false &&
      receipt.conservation_replay_complete === false;
  }
  return Number(receipt.imagine_skill_id) === 3921 &&
    receipt.component_id === "time-decree-external-cooldown-speed" &&
    receipt.exact_component_scalar_available === true &&
    receipt.exact_native_equation_available === true &&
    stableStringify(receipt.tier_values_percent) === stableStringify([10, 20, 30, 40, 50]) &&
    receipt.qualifying_skill_cooldown_category_map_complete === false &&
    receipt.recipient_cast_schedule_replay_complete === false &&
    receipt.conservation_replay_complete === false;
}

function isValidTargetVulnerabilityFormulaJoin(
  entry,
  targetVulnerabilityProofInput,
  closureSchemaVersion,
) {
  if (closureSchemaVersion < 46) return true;
  const receipt = entry?.target_vulnerability_formula ?? null;
  if (targetVulnerabilityProofInput === null) return receipt === null;
  if (Number(entry?.effect_id) !== 55228) return receipt === null;
  const expectedProofState = closureSchemaVersion >= 76
    ? "current-build-provider-lifecycle-controlled-baseline-components-and-target-rank-narrowed-third-party-module-and-intrinsic-current-hp-open-exact-scalar-open"
    : closureSchemaVersion >= 75
    ? "current-build-provider-lifecycle-action-edges-encounter-factor-and-complete-active-status-inventory-catalog-health-routes-narrowed-intrinsic-current-hp-and-exact-scalar-open"
    : closureSchemaVersion >= 74
    ? "current-build-provider-lifecycle-action-edges-encounter-factor-and-complete-active-status-inventory-current-hp-and-static-unmapped-triage-gaps-open-exact-scalar-open"
    : closureSchemaVersion >= 73
    ? "current-build-provider-lifecycle-action-edges-encounter-factor-and-complete-active-status-inventory-current-hp-and-unmapped-status-gaps-open-exact-scalar-open"
    : closureSchemaVersion >= 72
    ? "current-build-provider-lifecycle-action-edges-encounter-factor-selection-effective-combination-module-phy-boost-and-rorola-context-and-candidate-replay-conservation-proven-exact-scalar-open"
    : closureSchemaVersion >= 71
    ? "current-build-provider-lifecycle-action-edges-encounter-factor-selection-effective-combination-module-context-and-candidate-replay-conservation-proven-exact-scalar-open"
    : closureSchemaVersion >= 70
    ? "current-build-provider-lifecycle-action-edges-encounter-factor-selection-and-candidate-replay-conservation-proven-effective-combination-and-exact-scalar-open"
    : "current-build-provider-lifecycle-action-edges-and-candidate-replay-conservation-proven-exact-scalar-open";
  const common = receipt !== null && isValidFileDescriptor(receipt.input) &&
    stableStringify(receipt.input) === stableStringify(targetVulnerabilityProofInput) &&
    receipt.proof_state === expectedProofState &&
    Number(receipt.provider_owned_status_events) === 13842 &&
    Number(receipt.target_active_damage_samples) === 269731 &&
    Number(receipt.action_counterfactual_samples) === 56083 &&
    Number(receipt.provider_complete_present_absent_controlled_groups) === 0 &&
    Number(receipt.corrected_controlled_present_absent_groups) === 4 &&
    Number(receipt.corrected_controlled_sample_comparisons) === 7 &&
    receipt.controlled_observed_damage_delta_proven === true &&
    receipt.historical_plus_1000_candidate_compatible === true &&
    receipt.historical_plus_1000_candidate_unique === false &&
    stableStringify(receipt.compatible_total_factor_basis_points) ===
      stableStringify([{ minimum: 18455, maximum: 18456 }]) &&
    Number(receipt.damage_attr_id) === 2220329107 &&
    Number(receipt.candidate_ability_id) === 2203291 &&
    Number(receipt.candidate_replay_events) === 425 &&
    String(receipt.candidate_replay_delta) === "20300571" &&
    receipt.candidate_replay_conserved === true &&
    Number(receipt.deferred_multiple_provider_windows) === 215 &&
    receipt.historical_build_matches_current_build === false &&
    receipt.exact_current_build_scalar_proven === false &&
    receipt.exact_current_build_operation_order_proven === false &&
    receipt.stacking_and_multi_provider_split_proven === false &&
    receipt.integer_rounding_proven === false &&
    receipt.formula_authority === false && receipt.runtime_authority === false &&
    receipt.ui_display_authority === false && receipt.provider_rdps_credit_allowed === false &&
    Number(receipt.observed_damage_reassigned_to_provider) === 0 &&
    entry.blockers?.includes("exact-current-build-target-vulnerability-scalar-and-operation-order-open") &&
    entry.blockers?.includes("multi-provider-stacking-and-split-open") &&
    (closureSchemaVersion < 70 ||
      (closureSchemaVersion === 70
        ? receipt.encounter_factor_grades_proven === true &&
          receipt.duplicate_marksman_x10_selection_across_active_lines_observed === true &&
          receipt.encounter_factor_effective_combination_proven === false &&
          Number(receipt.observed_selection_compatible_additive_decompositions) === 0 &&
          entry.blockers?.includes("encounter-factor-effective-combination-open")
        : receipt.encounter_factor_grades_proven === true &&
          receipt.duplicate_marksman_x10_selection_across_active_lines_observed === true &&
          receipt.encounter_factor_effective_combination_proven === true &&
          Number(receipt.observed_selection_compatible_additive_decompositions) === 0 &&
          receipt.module_2104_level_and_scalar_proven === true &&
          Number(receipt.module_2104_selected_matching_provider_stacks) === 4 &&
          Number(receipt.module_2104_selected_configured_raw_damage_increase) === 1100 &&
          Number(receipt.module_2104_nonmatching_provider_instances_excluded) === 2 &&
          receipt.module_2104_transferable_provider_rdps_component === false &&
          receipt.module_2104_baseline_zone_raw_1000_proven === false &&
          receipt.module_2104_integer_rounding_model_unique === false &&
          (closureSchemaVersion < 72 ||
            (receipt.physical_boost_12550_identity_and_selected_raw_value_proven === true &&
             Number(receipt.physical_boost_12550_selected_raw_value) === 600 &&
             receipt.physical_boost_12550_same_state_in_present_and_absent_samples === true &&
             receipt.physical_boost_12550_exact_formula_stage_proven === false &&
             receipt.physical_boost_12550_same_target_vulnerability_bucket_as_effect_55228_proven === false &&
             receipt.rorola_3948_selected_profile_and_personal_values_proven === true &&
             Number(receipt.rorola_3948_selected_counter_stacks) === 11 &&
             Number(receipt.rorola_3948_diagnostic_personal_raw_value) === 1120 &&
             receipt.rorola_3948_external_provider_copy_excluded === true &&
             receipt.rorola_3948_transferable_provider_rdps_component === false &&
             receipt.rorola_3948_exact_formula_bucket_placement_proven === false &&
             Number(receipt.selected_pair_rorola_same_bucket_compatible_decompositions) === 0 &&
             Number(receipt.selected_pair_module_and_rorola_same_bucket_compatible_decompositions) === 0)) &&
          (closureSchemaVersion < 73 ||
            (receipt.selected_pair_only_target_attribute_difference_is_current_hp === true &&
             Number(receipt.selected_pair_current_hp_delta_present_minus_absent) === 1312178 &&
             receipt.selected_pair_exact_action_current_hp_independence_proven === false &&
             Number(receipt.selected_pair_shared_status_instances) === 120 &&
             Number(receipt.selected_pair_mapped_status_instances) === 75 &&
             Number(receipt.selected_pair_unmapped_status_instances) === 45 &&
             Number(receipt.selected_pair_unmapped_distinct_effect_ids) === 43 &&
             Number(receipt.selected_pair_mapped_damage_modifier_manifestations) === 11 &&
             receipt.selected_pair_all_shared_status_instances_mapped === false &&
             receipt.selected_pair_complete_static_inventory_proves_runtime_bucket_membership === false &&
             receipt.selected_pair_complete_static_inventory_proves_server_current_hp_independence === false)) &&
          (closureSchemaVersion < 74 ||
            (Number(receipt.selected_pair_unmapped_current_build_classification_entries) === 24 &&
             Number(receipt.selected_pair_unmapped_current_build_formula_term_entries) === 32 &&
             Number(receipt.selected_pair_unmapped_current_build_value_proof_entries) === 10 &&
             Number(receipt
               .selected_pair_unmapped_static_damage_or_stat_formula_zone_candidates) === 15 &&
             Number(receipt.selected_pair_unmapped_no_static_route_entries) === 11 &&
             Number(receipt
               .selected_pair_unmapped_target_locus_semantic_current_hp_candidates) === 0 &&
             receipt.selected_pair_unmapped_static_triage_is_runtime_formula_authority === false)) &&
          (closureSchemaVersion < 75 ||
            (Number(receipt.selected_pair_semantic_current_hp_target_locus_candidates) === 0 &&
             Number(receipt
               .selected_pair_active_state_dependent_direct_source_intersections) === 1 &&
             Number(receipt
               .selected_pair_defensive_owner_routes_excluded_from_selected_outgoing_damage) === 1 &&
             Number(receipt
               .selected_pair_outgoing_health_dependent_catalog_routes_remaining) === 0 &&
             receipt
               .selected_pair_intrinsic_server_action_target_current_hp_behavior_still_open === true)) &&
          (closureSchemaVersion < 76 ||
            (receipt.controlled_baseline_component_scan_proven === true &&
             receipt.controlled_baseline_target_monsters_all_normal_rank === true &&
             receipt.controlled_baseline_cuisine_elite_damage_clause_excluded === true &&
             receipt.controlled_baseline_third_party_module_transition_found === true &&
             String(receipt
               .controlled_baseline_third_party_module_provider_entity_uuid) === "190072160896" &&
             Number(receipt.controlled_baseline_third_party_module_observed_delta) === 15844 &&
             receipt
               .controlled_baseline_third_party_module_external_transfer_proven === false &&
             receipt
               .controlled_baseline_critical_output_invariant_across_three_target_hp_states ===
                true &&
             receipt
               .controlled_baseline_intrinsic_server_action_target_hp_behavior_globally_excluded ===
                false)) &&
          !entry.blockers?.includes("encounter-factor-effective-combination-open")));
  if (!common) return false;
  if (closureSchemaVersion < 50) {
    return receipt.protocol_promotion_allowed === false &&
      entry.blockers?.includes("protocol-pack-promotion-open");
  }
  const protocolBoundary = receipt.protocol_pack_promotion_allowed === true &&
    receipt.protocol_event_coverage_proven === true &&
    receipt.exact_pack_gap_free_segment_ordinary_damage_conservation_proven === true &&
    receipt.exact_pack_closed_lifecycle_canonical_replay_conservation_proven === false &&
    receipt.formula_specific_counterfactual_conservation_proven === false &&
    entry.blockers?.includes("canonical-replay-and-formula-specific-conservation-open") &&
    !entry.blockers?.includes("protocol-pack-promotion-open");
  if (!protocolBoundary) return false;
  if (closureSchemaVersion < 51) {
    return !entry.blockers?.includes("exact-party-skill-to-status-source-edge-open");
  }
  return Number(receipt.decoded_table_files_scanned) === 577 &&
    Number(receipt.decoded_table_bytes_scanned) === 172314811 &&
    Number(receipt.decoded_exact_id_occurrences_retained) === 1242 &&
    Number(receipt.provider_skill_id) === 2209 &&
    Number(receipt.provider_skill_effect_id) === 220901 &&
    receipt.exact_skill_to_skill_effect_edge_proven === true &&
    receipt.reviewed_skill_to_status_candidate_preserved === true &&
    receipt.exact_skill_to_status_edge_proven === false &&
    receipt.exact_typed_skill_or_skill_effect_reference_to_status_found === false &&
    receipt.status_exact_id_linked_scalar_candidate_found === false &&
    receipt.indirect_or_server_formula_ruled_out === false &&
    receipt.recipient_damage_measurement_action_is_provider_skill_identity === false &&
    entry.blockers?.includes("exact-party-skill-to-status-source-edge-open");
}

function isValidPartyEffectIdentityGate(entry, closureSchemaVersion) {
  const evidence = entry?.identity_evidence;
  const statusEvents = Number(entry?.status_events);
  const affectedIdentityProven =
    Number(evidence?.affected_entity_status_events) === statusEvents &&
    Number(evidence?.affected_entity_identity_unresolved_events) === 0 &&
    Number(evidence?.affected_entity_player_identity_events) +
      Number(evidence?.affected_entity_non_player_identity_events) === statusEvents;
  const affectedPlayerIdentityProven = affectedIdentityProven &&
    Number(evidence?.affected_entity_player_identity_events) === statusEvents;
  const externalPartyMembershipProven =
    Number(evidence?.external_source_affected_status_events) === 0 ||
    Number(evidence?.party_membership_proven_status_events) ===
      Number(evidence?.external_source_affected_status_events);
  const identityBlocker = "affected-entity-event-time-identity-open";
  const partyBlocker = "event-time-party-membership-for-external-affected-entities-open";
  const roleBlocker = "affected-entity-role-open";
  const expectedRole = resolvePartyEffectAffectedEntityRole({
    statusEvents,
    supportCategories: entry?.support_categories ?? [],
    identityEvidence: evidence,
    externalAffectedEntityPartyMembershipProvenForEveryStatusEvent:
      externalPartyMembershipProven,
    actorRelation: entry?.affected_entity_damage_actions,
    targetRelation: entry?.damage_actions_targeting_affected_entity,
    damageActionEdgeSummary: entry?.damage_action_edge_summary,
    requirePartyMembershipForRecipientRole: Number(closureSchemaVersion) < 52,
  });
  const roleValid = Number(closureSchemaVersion) < 49
    ? entry?.affected_entity_role_proven === false
    : entry?.affected_entity_role_proven === expectedRole.proven &&
      entry?.affected_entity_role_resolution === expectedRole.resolution &&
      entry?.affected_entity_role_requires_party_membership ===
        expectedRole.requires_party_membership &&
      (statusEvents === 0 || (expectedRole.proven
        ? !entry?.blockers?.includes(roleBlocker)
        : entry?.blockers?.includes(roleBlocker))) &&
      (statusEvents === 0 || (expectedRole.requires_party_membership && !externalPartyMembershipProven
        ? entry?.blockers?.includes(partyBlocker)
        : !entry?.blockers?.includes(partyBlocker)));
  return entry?.affected_entity_identity_proven_for_every_status_event ===
      affectedIdentityProven &&
    entry?.affected_entity_player_identity_proven_for_every_status_event ===
      affectedPlayerIdentityProven &&
    entry?.external_affected_entity_party_membership_proven_for_every_status_event ===
      externalPartyMembershipProven &&
    (statusEvents === 0 || (affectedIdentityProven
      ? !entry?.blockers?.includes(identityBlocker)
      : entry?.blockers?.includes(identityBlocker))) &&
    (Number(closureSchemaVersion) >= 49 || (statusEvents === 0 || (externalPartyMembershipProven
      ? !entry?.blockers?.includes(partyBlocker)
      : entry?.blockers?.includes(partyBlocker)))) &&
    roleValid;
}

function isValidPartyIdentityEvidence(evidence, auditSchemaVersion) {
  const countKeys = [
    "source_status_events", "source_player_identity_events",
    "source_non_player_identity_events", "source_identity_unresolved_events",
    "affected_entity_status_events", "affected_entity_player_identity_events",
    "affected_entity_non_player_identity_events", "affected_entity_identity_unresolved_events",
    "self_source_affected_status_events", "external_source_affected_status_events",
    "external_status_events_with_both_player_identities",
    "external_status_events_with_unresolved_identity",
    "party_membership_proven_status_events", "party_membership_unproven_status_events",
  ];
  const arrayKeys = [
    "source_actor_kinds", "affected_entity_actor_kinds", "source_character_ids",
    "affected_entity_character_ids", "source_class_ids", "affected_entity_class_ids",
  ];
  const rosterCountKeys = [
    "external_status_events_with_both_in_observed_party_roster",
    "external_status_events_with_roster_evidence_but_lifecycle_coverage_open",
  ];
  const rosterCountsAbsent = rosterCountKeys.every((key) => evidence?.[key] === undefined);
  const rosterCountsValid = rosterCountKeys.every((key) =>
    Number.isSafeInteger(Number(evidence?.[key])) && Number(evidence[key]) >= 0);
  const teamCountKeys = [
    "external_status_events_with_matching_last_observed_team_id",
    "external_status_events_with_mismatching_last_observed_team_ids",
    "external_status_events_with_unresolved_last_observed_team_id",
    "external_status_events_with_team_id_evidence_but_protocol_coverage_open",
  ];
  const requiresTeamEvidence = Number.isFinite(Number(auditSchemaVersion))
    ? Number(auditSchemaVersion) >= 6
    : teamCountKeys.some((key) => evidence?.[key] !== undefined) ||
      evidence?.matching_last_observed_team_ids !== undefined;
  const teamEvidenceValid = !requiresTeamEvidence ||
    (teamCountKeys.every((key) =>
      Number.isSafeInteger(Number(evidence?.[key])) && Number(evidence[key]) >= 0) &&
      Array.isArray(evidence?.matching_last_observed_team_ids) &&
      Number(evidence.external_status_events_with_matching_last_observed_team_id) ===
        Number(evidence.external_status_events_with_team_id_evidence_but_protocol_coverage_open) &&
      Number(evidence.external_status_events_with_matching_last_observed_team_id) +
        Number(evidence.external_status_events_with_mismatching_last_observed_team_ids) +
        Number(evidence.external_status_events_with_unresolved_last_observed_team_id) ===
        Number(evidence.external_source_affected_status_events));
  return evidence !== null && typeof evidence === "object" &&
    countKeys.every((key) => Number.isSafeInteger(Number(evidence[key])) && Number(evidence[key]) >= 0) &&
    (rosterCountsAbsent || rosterCountsValid) &&
    teamEvidenceValid &&
    arrayKeys.every((key) => Array.isArray(evidence[key])) &&
    Number(evidence.party_membership_proven_status_events) === 0;
}

function summarizePartyIdentityEvidence(evidence, auditSchemaVersion) {
  return {
    source_status_events: Number(evidence.source_status_events),
    source_player_identity_events: Number(evidence.source_player_identity_events),
    source_non_player_identity_events: Number(evidence.source_non_player_identity_events),
    source_identity_unresolved_events: Number(evidence.source_identity_unresolved_events),
    affected_entity_status_events: Number(evidence.affected_entity_status_events),
    affected_entity_player_identity_events: Number(evidence.affected_entity_player_identity_events),
    affected_entity_non_player_identity_events: Number(evidence.affected_entity_non_player_identity_events),
    affected_entity_identity_unresolved_events: Number(evidence.affected_entity_identity_unresolved_events),
    self_source_affected_status_events: Number(evidence.self_source_affected_status_events),
    external_source_affected_status_events: Number(evidence.external_source_affected_status_events),
    external_status_events_with_both_player_identities:
      Number(evidence.external_status_events_with_both_player_identities),
    external_status_events_with_unresolved_identity:
      Number(evidence.external_status_events_with_unresolved_identity),
    external_status_events_with_both_in_observed_party_roster:
      Number(evidence.external_status_events_with_both_in_observed_party_roster ?? 0),
    external_status_events_with_roster_evidence_but_lifecycle_coverage_open:
      Number(evidence.external_status_events_with_roster_evidence_but_lifecycle_coverage_open ?? 0),
    ...(Number(auditSchemaVersion) >= 6 ? {
      external_status_events_with_matching_last_observed_team_id:
        Number(evidence.external_status_events_with_matching_last_observed_team_id),
      external_status_events_with_mismatching_last_observed_team_ids:
        Number(evidence.external_status_events_with_mismatching_last_observed_team_ids),
      external_status_events_with_unresolved_last_observed_team_id:
        Number(evidence.external_status_events_with_unresolved_last_observed_team_id),
      external_status_events_with_team_id_evidence_but_protocol_coverage_open:
        Number(evidence.external_status_events_with_team_id_evidence_but_protocol_coverage_open),
      matching_last_observed_team_ids:
        uniqueSorted(evidence.matching_last_observed_team_ids),
    } : {}),
    party_membership_proven_status_events: Number(evidence.party_membership_proven_status_events),
    party_membership_unproven_status_events: Number(evidence.party_membership_unproven_status_events),
    source_actor_kinds: uniqueSorted(evidence.source_actor_kinds),
    affected_entity_actor_kinds: uniqueSorted(evidence.affected_entity_actor_kinds),
    source_character_ids: uniqueSorted(evidence.source_character_ids),
    affected_entity_character_ids: uniqueSorted(evidence.affected_entity_character_ids),
    source_class_ids: uniqueSortedNumbers(evidence.source_class_ids),
    affected_entity_class_ids: uniqueSortedNumbers(evidence.affected_entity_class_ids),
  };
}

function summarizePartyDamageRelation(relation) {
  return {
    event_count: Number(relation.event_count),
    amount: String(relation.amount),
    ability_ids: uniqueSortedNumbers(relation.ability_ids),
    damage_source_actor_count: relation.damage_source_actor_ids.length,
    damage_target_actor_count: relation.damage_target_actor_ids.length,
  };
}

function isValidPartyDamageRelationSummary(relation) {
  return Number.isSafeInteger(Number(relation?.event_count)) && Number(relation.event_count) >= 0 &&
    /^\d+$/.test(String(relation?.amount ?? "")) && Array.isArray(relation?.ability_ids) &&
    Number.isSafeInteger(Number(relation?.damage_source_actor_count)) &&
    Number(relation.damage_source_actor_count) >= 0 &&
    Number.isSafeInteger(Number(relation?.damage_target_actor_count)) &&
    Number(relation.damage_target_actor_count) >= 0;
}

function isValidPartyDamageActionEdgeSummary(summary) {
  const keys = [
    "edge_count",
    "effect_target_as_damage_actor_edges",
    "effect_target_as_damage_target_edges",
    "effect_target_as_damage_actor_event_references",
    "effect_target_as_damage_target_event_references",
  ];
  return summary !== null && typeof summary === "object" &&
    keys.every((key) => Number.isSafeInteger(Number(summary[key])) && Number(summary[key]) >= 0) &&
    Number(summary.edge_count) === Number(summary.effect_target_as_damage_actor_edges) +
      Number(summary.effect_target_as_damage_target_edges);
}

function verify(input) {
  requireFile(input, "rDPS proof closure report");
  const report = readJson(input, "rDPS proof closure report");
  if (![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 35, 39, 40, 41, 42, 43, 44, 45, 46, 49, 50, 51, 52, 55, 56, 57, 58, 59, 60, PROOF_CLOSURE_SCHEMA_VERSION].includes(report.schema_version)) {
    throw new Error(`Unsupported closure schema ${report.schema_version}`);
  }
  if (report.content_sha256 !== contentHash(report)) throw new Error("rDPS proof closure content hash mismatch");
  if (!report.policy?.every_manifest_obligation_is_preserved_exactly_once ||
    !report.policy?.every_packet_observed_runtime_effect_is_preserved_exactly_once ||
    !report.policy?.unresolved_evidence_is_never_hidden) {
    throw new Error("rDPS proof closure policy is unsafe");
  }
  if (Number(report.schema_version) >= 79 &&
    report.policy?.counterfactual_transfer_gates_always_require_complete_formula_inputs !== true) {
    throw new Error("rDPS proof closure counterfactual formula-input policy is unsafe");
  }
  if (report.policy?.shared_proof_receipts_never_close_downstream_runtime_or_conservation_gates !== true ||
    report.policy?.canonical_runtime_input_route_proof_receipts_never_close_provider_projection_or_conservation_gates !== true ||
    report.policy?.registry_only_proof_routes_remain_open_without_fabricated_runtime_obligations !== true ||
    report.policy?.exact_runtime_replay_may_promote_only_its_conserved_unambiguous_subset !== true ||
    report.policy?.exact_runtime_subset_promotion_is_not_full_effect_or_family_resolution !== true ||
    report.policy?.component_level_current_build_routing_precedes_coarse_runtime_effect_classification !== true ||
    report.policy?.ambiguous_provider_windows_remain_deferred !== true) {
    throw new Error("rDPS proof closure shared-receipt policy is unsafe");
  }
  if (Number(report.schema_version) >= 13 &&
    report.policy?.exact_build_damage_surface_grouping_never_grants_formula_runtime_or_ui_authority !== true) {
    throw new Error("rDPS proof closure damage-surface grouping policy is unsafe");
  }
  if (Number(report.schema_version) >= 14 &&
    report.policy?.bounded_counterfactual_processing_overage_or_missing_pair_never_grants_authority !== true) {
    throw new Error("rDPS proof closure bounded counterfactual policy is unsafe");
  }
  if (Number(report.schema_version) >= 24 &&
    report.policy
      ?.critical_damage_factor_interpretation_receipts_never_grant_formula_runtime_or_ui_authority !== true) {
    throw new Error("rDPS proof closure critical-damage interpretation receipt policy is unsafe");
  }
  if (Number(report.schema_version) >= 25 &&
    report.policy
      ?.critical_damage_runtime_gate_receipts_never_grant_formula_runtime_or_ui_authority !== true) {
    throw new Error("rDPS proof closure critical-damage runtime gate receipt policy is unsafe");
  }
  if ((report.counterfactual_frontier_results?.length ?? 0) > 0 &&
    report.policy?.counterfactual_frontier_evidence_never_grants_formula_or_runtime_authority !== true) {
    throw new Error("rDPS proof closure counterfactual-frontier policy is unsafe");
  }
  if ((report.inputs?.provider_ownership_proofs?.length ?? 0) > 0 &&
    report.policy?.provider_ownership_evidence_never_grants_formula_or_runtime_authority !== true) {
    throw new Error("rDPS proof closure provider-ownership policy is unsafe");
  }
  if ((report.inputs?.integer_transform_constraints?.length ?? 0) > 0 &&
    report.policy?.integer_transform_constraints_never_grant_formula_or_runtime_authority !== true) {
    throw new Error("rDPS proof closure integer-transform policy is unsafe");
  }
  if ((report.inputs?.component_scalar_proofs?.length ?? 0) > 0 &&
    report.policy
      ?.component_static_scalar_proofs_never_grant_formula_projection_runtime_or_conservation_authority !== true) {
    throw new Error("rDPS proof closure component-static-scalar policy is unsafe");
  }
  const componentScalars = uniqueIndex(
    report.component_scalar_frontier_results ?? [],
    "effect_id",
    "component-scalar frontier result",
  );
  if (componentScalars.size !== Number(report.summary?.component_scalar_frontier_results ?? 0) ||
    componentScalars.size !== (report.inputs?.component_scalar_proofs?.length ?? 0) ||
    [...componentScalars.values()].some((entry) =>
      !String(entry?.proof?.path ?? "") || Number(entry?.proof?.bytes) <= 0 ||
      !/^[0-9a-f]{64}$/.test(String(entry?.proof?.sha256 ?? "")) ||
      entry?.exact_damage_projection_proven !== false ||
      entry?.exact_operation_order_proven !== false ||
      entry?.exact_integer_rounding_proven !== false ||
      entry?.packet_conservation_proven !== false || entry?.formula_authority !== false ||
      entry?.runtime_authority !== false || entry?.provider_rdps_credit_allowed !== false ||
      (Number(entry?.proof_schema_version) >= 24 &&
        !isValidTargetEffectFormulaProof(entry?.target_effect_formula_proof)) ||
      (Number(entry?.proof_schema_version) >= 26 &&
        (Number(entry?.counterfactual_discriminants?.observed_baseline_curve
          ?.same_input_status_invariance?.compatible_target_status_state_ids) !== 20 ||
          Number(entry?.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.common_effect_ids_across_all_compatible_rows?.length) !== 78 ||
          Number(entry?.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.varying_effect_ids_across_all_compatible_rows?.length) !== 36 ||
          Number(entry?.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.isolated_single_effect_toggle_count) !== 1 ||
          entry?.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.common_target_status_confounders_remain !== true ||
          entry?.counterfactual_discriminants?.observed_baseline_curve
            ?.same_input_status_invariance?.target_status_control_proven !== false))
      || (Number(entry?.proof_schema_version) >= 27 &&
        (!isValidRlogTransitionStagedResidualFrontier(entry?.rlog_transition_counterfactual_audit) ||
          !entry?.blockers?.includes(
            "closest-transition-pair-equal-output-five-status-differences-incomplete-snapshots",
          )))
      || (Number(entry?.proof_schema_version) >= 28 &&
        (!isValidLuckyPacketComponentReceipt(entry?.lucky_packet_component_proof) ||
          !entry?.blockers?.includes(
            "closest-transition-damage-component-proven-lucky-operator-mitigation-route-open",
          )))
      || (Number(entry?.proof_schema_version) >= 29 &&
        (!isValidMAttackLuckyMitigationReceipt(entry?.mattack_lucky_mitigation_diagnostic) ||
          !entry?.blockers?.includes(
            "mattack-lucky-packets-have-no-observed-target-mitigation-axis-remote-packets-not-required",
          )))
      || (Number(entry?.proof_schema_version) >= 30 &&
        (!isValidAttackLuckyMitigationReceipt(entry?.attack_lucky_mitigation_diagnostic) ||
          !isValidAttackLuckyComponentRows(entry?.lucky_packet_component_proof?.attack_lucky_rows) ||
          !entry?.blockers?.includes(
            "attack-lucky-packets-have-no-observed-target-mitigation-axis-route-remains-unresolved",
          )))
      || (Number(entry?.proof_schema_version) === 31 &&
        (!isValidLuckyParentMultiplierReceipt(entry?.lucky_parent_multiplier_proof) ||
          !entry?.blockers?.includes(
            "lucky-parent-relation-complete-two-obvious-attribute-12530-multiplier-formulas-rejected",
          )))
      || (Number(entry?.proof_schema_version) === 32 &&
        (!isValidGroupedLuckyParentMultiplierReceipt(entry?.lucky_parent_multiplier_proof) ||
          !entry?.blockers?.includes(
            "lucky-parent-relation-complete-three-recorded-multiplier-candidates-rejected-grouped-residual-positive",
          )))
      || (Number(entry?.proof_schema_version) >= 33 &&
        (!isValidExhaustiveLocalSourceAttributeLuckyReceipt(entry?.lucky_parent_multiplier_proof) ||
          !entry?.blockers?.includes(
            "lucky-parent-relation-complete-224-local-single-attribute-multiplier-candidates-rejected-multi-input-formula-open",
          )))
    )) {
    throw new Error("rDPS proof closure component-scalar frontier is inconsistent");
  }
  const openComponentScalarProjections = [...componentScalars.values()].filter(
    (entry) => !entry.exact_damage_projection_proven,
  ).length;
  const componentScalarProviderCreditAllowed = [...componentScalars.values()].filter(
    (entry) => entry.provider_rdps_credit_allowed,
  ).length;
  if (openComponentScalarProjections !==
      Number(report.summary?.component_scalar_open_damage_projections ?? 0) ||
    componentScalarProviderCreditAllowed !==
      Number(report.summary?.component_scalar_provider_credit_allowed ?? 0)) {
    throw new Error("rDPS proof closure component-scalar summary is inconsistent");
  }
  const supportEffects = uniqueIndex(
    report.support_effect_frontier_results ?? [],
    "effect_id",
    "support-effect frontier result",
  );
  if (supportEffects.size > 0 &&
    report.policy
      ?.support_effect_proof_receipts_never_grant_opportunity_runtime_or_ui_authority !== true) {
    throw new Error("rDPS proof closure support-effect receipt policy is unsafe");
  }
  if (supportEffects.size !== Number(report.summary?.support_effect_frontier_results ?? 0) ||
    supportEffects.size !== (report.inputs?.support_effect_proofs?.length ?? 0) ||
    [...supportEffects.values()].some((entry) =>
      !isValidSupportEffectFrontierResult(entry, Number(report.schema_version)))) {
    throw new Error("rDPS proof closure support-effect frontier is inconsistent");
  }
  if (Number(report.schema_version) >= 24) {
    const inspiration = supportEffects.get("2202041") ?? null;
    const requiresInterpretationReceipt = Number(inspiration?.proof_schema_version ?? 0) >= 17;
    const receiptInput = report.inputs?.critical_damage_factor_interpretation_proof ?? null;
    if (requiresInterpretationReceipt
      ? (!isValidFileDescriptor(receiptInput) ||
        stableStringify(receiptInput) !==
          stableStringify(inspiration.critical_damage_factor_interpretation_proof))
      : receiptInput !== null) {
      throw new Error("rDPS proof closure critical-damage interpretation receipt is inconsistent");
    }
  }
  if (Number(report.schema_version) >= 28) {
    const inspiration = supportEffects.get("2202041") ?? null;
    const requiresControlledPairReceipt = Number(inspiration?.proof_schema_version ?? 0) >= 17;
    const discriminantInput =
      report.inputs?.critical_factor_controlled_pair_discriminant ?? null;
    if (requiresControlledPairReceipt
      ? (!isValidFileDescriptor(discriminantInput) ||
        stableStringify(discriminantInput) !== stableStringify(
          inspiration.critical_factor_controlled_pair_discriminant,
        ) ||
        !isValidControlledPairDiscriminantAudit(
          inspiration.critical_factor_controlled_pair_audit,
          Number(report.schema_version) >= 29 ? 2 : 1,
        ))
      : discriminantInput !== null) {
      throw new Error("rDPS proof closure critical-factor controlled-pair receipt is inconsistent");
    }
  }
  const exactSupportTransforms = [...supportEffects.values()].filter(
    (entry) => entry.exact_stat_transform_proven,
  ).length;
  const openSupportOpportunityFormulas = [...supportEffects.values()].filter(
    (entry) => !entry.opportunity_counterfactual_proven,
  ).length;
  const supportProviderCreditAllowed = [...supportEffects.values()].filter(
    (entry) => entry.provider_rdps_credit_allowed,
  ).length;
  if (exactSupportTransforms !== Number(report.summary?.support_effect_exact_stat_transforms ?? 0) ||
    openSupportOpportunityFormulas !==
      Number(report.summary?.support_effect_open_opportunity_formulas ?? 0) ||
    supportProviderCreditAllowed !==
      Number(report.summary?.support_effect_provider_credit_allowed ?? 0) ||
    report.summary?.support_effect_frontier_complete !==
      [...supportEffects.values()].every((entry) => entry.provider_rdps_credit_allowed)) {
    throw new Error("rDPS proof closure support-effect summary is inconsistent");
  }
  if (Number(report.schema_version) >= 77) {
    if (report.policy
      ?.life_wave_trigger_receipts_never_grant_formula_runtime_ui_or_credit_authority !== true) {
      throw new Error("rDPS proof closure Life Wave receipt policy is unsafe");
    }
    const input = report.inputs?.life_wave_trigger_proof ?? null;
    const frontier = report.life_wave_trigger_frontier ?? null;
    const receiptCount = input === null ? 0 : 1;
    const complete = frontier?.provider_rdps_credit_allowed === true;
    if (Number(report.summary?.life_wave_trigger_proof_receipts) !== receiptCount ||
      report.summary?.life_wave_trigger_frontier_complete !== complete ||
      (input === null
        ? (frontier !== null ||
          Number(report.summary?.life_wave_refresh_activations) !== 0 ||
          Number(report.summary?.life_wave_reprocs_before_expiry) !== 0 ||
          Number(report.summary?.life_wave_unique_external_heal_candidate_activations) !== 0 ||
          Number(report.summary?.life_wave_unique_self_heal_candidate_activations) !== 0 ||
          Number(report.summary?.life_wave_ambiguous_heal_candidate_activations) !== 0 ||
          Number(report.summary?.life_wave_no_heal_candidate_activations) !== 0 ||
          !report.production_readiness?.blockers?.includes("life-wave-trigger-proof-missing"))
        : (!isValidFileDescriptor(input) || !frontier ||
          stableStringify(frontier.proof) !== stableStringify(input) ||
          Number(frontier.module_effect_id) !== 2404 ||
          Number(frontier.parent_effect_id) !== 2302420 ||
          Number(frontier.effect_id) !== 2302421 ||
          frontier.refresh_timer_reset_proven !== true ||
          frontier.remote_character_snapshot_required !== false ||
          frontier.per_refresh_trigger_provider_proven !== false ||
          frontier.selected_secondary_lane_proven !== false ||
          frontier.exact_damage_counterfactual_proven !== false ||
          frontier.exact_integer_conservation_proven !== false ||
          frontier.formula_authority !== false || frontier.runtime_authority !== false ||
          frontier.ui_authority !== false || frontier.provider_rdps_credit_allowed !== false ||
          Number(frontier.activation_count) <= 0 ||
          Number(frontier.same_instance_reproc_before_expiry_count) <= 0 ||
          Number(report.summary?.life_wave_refresh_activations) !==
            Number(frontier.activation_count) ||
          Number(report.summary?.life_wave_reprocs_before_expiry) !==
            Number(frontier.same_instance_reproc_before_expiry_count) ||
          Number(report.summary?.life_wave_unique_external_heal_candidate_activations) !==
            Number(frontier.unique_external_heal_candidate_activations) ||
          Number(report.summary?.life_wave_unique_self_heal_candidate_activations) !==
            Number(frontier.unique_self_heal_candidate_activations) ||
          Number(report.summary?.life_wave_ambiguous_heal_candidate_activations) !==
            Number(frontier.ambiguous_heal_candidate_activations) ||
          Number(report.summary?.life_wave_no_heal_candidate_activations) !==
            Number(frontier.no_heal_candidate_activations) ||
          !report.production_readiness?.blockers?.includes(
            Number(report.schema_version) >= 78
              ? (report.inputs?.life_wave_remote_inference_proof
                ? "life-wave-open:2302421:packet-trigger-remote-formula-inference-and-conservation"
                : "life-wave-remote-inference-proof-missing")
              : "life-wave-open:2302421:packet-trigger-remote-paired-output-conservation",
          )))) {
      throw new Error("rDPS proof closure Life Wave trigger frontier is inconsistent");
    }
  }
  if (Number(report.schema_version) >= 78) {
    if (report.policy
      ?.life_wave_remote_inference_receipts_never_grant_exact_formula_runtime_ui_or_credit_authority !== true) {
      throw new Error("rDPS proof closure Life Wave remote inference policy is unsafe");
    }
    const triggerInput = report.inputs?.life_wave_trigger_proof ?? null;
    const input = report.inputs?.life_wave_remote_inference_proof ?? null;
    const frontier = report.life_wave_remote_inference_frontier ?? null;
    const receiptCount = input === null ? 0 : 1;
    const complete = frontier?.provider_rdps_credit_allowed === true;
    const missingExpected = triggerInput !== null && input === null;
    if (Number(report.summary?.life_wave_remote_inference_proof_receipts) !== receiptCount ||
      report.summary?.life_wave_remote_inference_frontier_complete !== complete ||
      report.production_readiness?.life_wave_remote_inference_frontier_complete !== complete ||
      (input === null
        ? (frontier !== null ||
          Number(report.summary?.life_wave_remote_inference_damage_rows) !== 0 ||
          Number(report.summary?.life_wave_remote_inference_active_damage_rows) !== 0 ||
          Number(report.summary?.life_wave_remote_inference_inactive_damage_rows) !== 0 ||
          Number(report.summary?.life_wave_remote_inference_exact_direct_pairs) !== 0 ||
          Number(report.summary
            ?.life_wave_remote_inference_unpaired_external_active_damage_rows) !== 0 ||
          (missingExpected && !report.production_readiness?.blockers?.includes(
            "life-wave-remote-inference-proof-missing",
          )))
        : (!isValidFileDescriptor(input) || !frontier || triggerInput === null ||
          stableStringify(frontier.proof) !== stableStringify(input) ||
          Number(frontier.effect_id) !== 2302421 ||
          frontier.remote_character_snapshot_required !== false ||
          frontier.remote_loadout_required !== false ||
          frontier.cross_vantage_exact_evidence_preferred !== true ||
          frontier.inferred_display_path_required !== true ||
          frontier.selected_secondary_lane_proven !== false ||
          frontier.exact_damage_counterfactual_proven !== false ||
          frontier.exact_integer_conservation_proven !== false ||
          frontier.formula_authority !== false || frontier.runtime_authority !== false ||
          frontier.ui_authority !== false || frontier.provider_rdps_credit_allowed !== false ||
          Number(frontier.activation_count) <= 0 || Number(frontier.wearer_count) <= 0 ||
          Number(frontier.damage_rows_for_life_wave_wearers) !==
            Number(frontier.active_damage_rows) + Number(frontier.inactive_damage_rows) ||
          Number(report.summary?.life_wave_remote_inference_damage_rows) !==
            Number(frontier.damage_rows_for_life_wave_wearers) ||
          Number(report.summary?.life_wave_remote_inference_active_damage_rows) !==
            Number(frontier.active_damage_rows) ||
          Number(report.summary?.life_wave_remote_inference_inactive_damage_rows) !==
            Number(frontier.inactive_damage_rows) ||
          Number(report.summary?.life_wave_remote_inference_exact_direct_pairs) !==
            Number(frontier.accepted_direct_pair_count) ||
          Number(report.summary
            ?.life_wave_remote_inference_unpaired_external_active_damage_rows) !==
            Number(frontier.unpaired_external_active_damage_rows) ||
          !report.production_readiness?.blockers?.includes(
            "life-wave-open:2302421:packet-trigger-remote-formula-inference-and-conservation",
          )))) {
      throw new Error("rDPS proof closure Life Wave remote inference frontier is inconsistent");
    }
  }
  if (Number(report.schema_version) >= 30) {
    if (report.policy?.party_skill_static_frontier_never_fabricates_packet_obligations !== true ||
      report.policy
        ?.party_skill_static_evidence_never_grants_formula_runtime_or_ui_authority !== true) {
      throw new Error("rDPS proof closure party-skill frontier policy is unsafe");
    }
    const frontier = report.party_skill_static_frontier;
    const skillResults = uniqueIndex(
      frontier?.skill_results ?? [],
      "skill_id",
      "party-skill static frontier result",
    );
    const rogueResults = uniqueIndex(
      frontier?.rogue_entry_results ?? [],
      "entry_id",
      "party-entry static frontier result",
    );
    const allResults = [...skillResults.values(), ...rogueResults.values()];
    const complete = allResults.every((entry) => entry.provider_rdps_credit_allowed === true);
    if (!isValidFileDescriptor(report.inputs?.party_skill_static_closure) ||
      stableStringify(frontier?.input) !==
        stableStringify(report.inputs.party_skill_static_closure) ||
      frontier?.proof_state !== "exact-build-static-party-frontier-runtime-proof-open" ||
      Number(frontier?.packet_obligations_fabricated) !== 0 ||
      Number(frontier?.unresolved_evidence_hidden) !== 0 ||
      frontier?.complete !== complete ||
      skillResults.size !== Number(report.summary?.party_skill_static_frontier_results) ||
      rogueResults.size !== Number(report.summary?.party_rogue_entry_static_frontier_results) ||
      allResults.some((entry) =>
        entry?.formula_authority !== false || entry?.runtime_authority !== false ||
        entry?.ui_display_authority !== false ||
        entry?.provider_rdps_credit_allowed !== false ||
        !Array.isArray(entry?.proof_obligations) || entry.proof_obligations.length === 0
      ) ||
      Number(report.summary?.party_skill_static_frontier_provider_credit_allowed) !==
        [...skillResults.values()].filter((entry) => entry.provider_rdps_credit_allowed).length ||
      Number(report.summary?.party_rogue_entry_static_frontier_provider_credit_allowed) !==
        [...rogueResults.values()].filter((entry) => entry.provider_rdps_credit_allowed).length ||
      report.summary?.party_skill_static_frontier_complete !== complete) {
      throw new Error("rDPS proof closure party-skill frontier is inconsistent");
    }
    for (const entry of skillResults.values()) {
      const blocker = `party-skill-open:${entry.skill_id}:formula-scope-or-runtime-proof`;
      if (!entry.provider_rdps_credit_allowed &&
        !report.production_readiness?.blockers?.includes(blocker)) {
        throw new Error(`Party-skill readiness blocker is missing for ${entry.skill_id}`);
      }
    }
    for (const entry of rogueResults.values()) {
      const blocker = `party-entry-open:${entry.entry_id}:formula-scope-or-runtime-proof`;
      if (!entry.provider_rdps_credit_allowed &&
        !report.production_readiness?.blockers?.includes(blocker)) {
        throw new Error(`Party-entry readiness blocker is missing for ${entry.entry_id}`);
      }
    }
  }
  if (Number(report.schema_version) >= 31) {
    if (report.policy?.party_effect_window_evidence_never_assumes_affected_entity_allegiance !== true ||
      report.policy?.party_effect_window_evidence_never_grants_formula_runtime_or_ui_authority !== true ||
      report.policy?.party_effect_window_evidence_preserves_both_damage_relationships !== true ||
      (Number(report.schema_version) >= 32 &&
        report.policy?.party_effect_window_evidence_preserves_neutral_action_links !== true) ||
      (Number(report.schema_version) >= 35 &&
        report.policy
          ?.party_effect_packet_origin_edges_are_exact_build_gated_and_non_authoritative !== true) ||
      (Number(report.schema_version) >= 36 &&
        report.policy
          ?.party_effect_window_evidence_preserves_allegiance_neutral_actor_ability_target_edges !== true) ||
      (Number(report.schema_version) >= 37 &&
        report.policy
          ?.party_effect_provider_ownership_receipts_require_exact_cohort_and_event_count_match !== true) ||
      (Number(report.schema_version) >= 38 &&
        report.policy?.party_effect_identity_and_party_membership_gates_are_separate !== true) ||
      (Number(report.schema_version) >= 49 &&
        report.policy
          ?.party_effect_role_proof_uses_relationship_and_event_time_entity_evidence_not_localized_allegiance !== true) ||
      (Number(report.schema_version) >= 39 &&
        report.policy
          ?.status_event_season_context_receipts_reject_future_profile_backfill !== true) ||
      (Number(report.schema_version) >= 40 &&
        report.policy?.status_event_season_context_gap_routes_are_evidence_not_authority !== true) ||
      (Number(report.schema_version) >= 41 &&
        report.policy
          ?.season_state_mutation_surface_receipts_never_grant_event_time_authority !== true) ||
      (Number(report.schema_version) >= 42 &&
        report.policy
          ?.static_reconnect_lifecycle_receipts_never_grant_event_time_logout_exclusion !== true) ||
      (Number(report.schema_version) >= 43 &&
        report.policy
          ?.observed_stacking_frontier_receipts_never_grant_server_arbitration_or_rounding_authority !== true) ||
      (Number(report.schema_version) >= 44 &&
        report.policy
          ?.native_action_speed_and_conditional_capacity_receipts_never_grant_opportunity_rounding_or_credit_authority !== true) ||
      (Number(report.schema_version) >= 45 &&
        report.policy
          ?.imagine_static_scalar_receipts_never_grant_event_time_tier_damage_stage_conservation_or_credit_authority !== true) ||
      (Number(report.schema_version) >= 46 &&
        report.policy
          ?.historical_target_vulnerability_pairs_never_grant_current_build_formula_runtime_or_ui_authority !== true) ||
      (Number(report.schema_version) >= 47 &&
        report.policy
          ?.imagine_status_attribute_tier_receipts_are_occurrence_scoped_and_never_propagate_across_time_or_recipients !== true) ||
      (Number(report.schema_version) >= 48 &&
        report.policy
          ?.imagine_tier_window_counterfactual_input_receipts_retain_neutral_actions_without_granting_damage_formula_or_credit_authority !== true) ||
      (Number(report.schema_version) >= 53 &&
        report.policy
          ?.fatal_spiral_damage_stage_frontier_receipts_never_grant_formula_rounding_conservation_runtime_or_ui_authority !== true) ||
      (Number(report.schema_version) >= 58 &&
        report.policy
          ?.blocked_hidden_damage_control_surfaces_are_acquisition_evidence_not_authorization_or_formula_authority !== true) ||
      (Number(report.schema_version) >= 59 &&
        report.policy
          ?.unproven_training_scene_routes_never_grant_acquisition_formula_runtime_or_ui_authority !== true) ||
      (Number(report.schema_version) >= 60 &&
        report.policy
          ?.exact_native_immediate_search_receipts_never_exclude_computed_indirect_table_driven_or_protected_consumers !== true) ||
      (Number(report.schema_version) >= 61 &&
        report.policy
          ?.recovered_partial_prefix_receipts_never_grant_source_capture_integrity_formula_runtime_or_ui_authority !== true) ||
      (Number(report.schema_version) >= 62 &&
        report.policy
          ?.generic_getter_and_exact_pointer_slot_receipts_never_exclude_runtime_derived_indexed_metadata_or_protected_consumers !== true) ||
      (Number(report.schema_version) >= 63 &&
        report.policy
          ?.sealed_candidate_readiness_receipts_never_grant_controlled_pair_formula_runtime_or_ui_authority !== true) ||
      (Number(report.schema_version) >= 64 &&
        report.policy
          ?.configured_endpoint_attribute_family_diagnostics_never_grant_formula_input_operation_order_runtime_or_ui_authority !== true) ||
      (Number(report.schema_version) >= 65 &&
        report.policy
          ?.exact_build_spatial_attribute_names_are_evidence_only_and_spatial_dimensions_remain_matched !== true) ||
      (Number(report.schema_version) >= 66 &&
        (report.policy
          ?.exact_build_action_selectors_and_packet_source_routes_never_grant_formula_runtime_or_ui_authority !== true ||
          report.policy?.packet_source_route_mismatches_remain_visible_and_unresolved !== true)) ||
      (Number(report.schema_version) >= 67 &&
        (report.policy
          ?.relative_spatial_relation_tolerances_are_diagnostic_and_never_promotion_rules !== true ||
          report.policy
            ?.relative_spatial_relation_equality_never_proves_all_spatial_damage_inputs_equal !== true)) ||
      (Number(report.schema_version) >= 68 &&
        (report.policy
          ?.future_fake_bullet_timeline_capture_never_backfills_historical_observations_or_provider_ownership !== true ||
          report.policy
            ?.fake_bullet_aoi_container_never_becomes_provider_without_exact_relation_proof !== true))) {
      throw new Error("rDPS proof closure party-effect window policy is unsafe");
    }
    const frontier = report.party_effect_window_frontier;
    const effects = uniqueIndex(
      frontier?.effect_results ?? [],
      "effect_id",
      "party-effect window frontier result",
    );
    const observed = [...effects.values()].filter((entry) => Number(entry.status_events) > 0);
    const complete = [...effects.values()].every(
      (entry) => entry.provider_rdps_credit_allowed === true,
    );
    const providerOwnershipProven = [...effects.values()].filter(
      (entry) => entry.provider_ownership_proven_for_every_status_event === true,
    ).length;
    if (!isValidFileDescriptor(report.inputs?.party_effect_window_audit) ||
      stableStringify(frontier?.input) !==
        stableStringify(report.inputs.party_effect_window_audit) ||
      frontier?.proof_state !== (Number(report.schema_version) >= 32
        ? "exact-build-canonical-party-effect-windows-identities-and-neutral-action-links-observed-semantics-open"
        : "exact-build-canonical-party-effect-windows-observed-semantics-open") ||
      frontier?.affected_entity_allegiance_assumed !== false ||
      Number(frontier?.remote_cast_rows_synthesized) !== 0 ||
      (Number(report.schema_version) >= 35 &&
        (String(frontier?.fight_source_enum_build) !== String(report.game_build) ||
          frontier?.packet_origin_edges_are_skill_to_buff_edges !== false ||
          frontier?.packet_origin_edges_are_provider_ownership_authority !== false ||
          frontier?.packet_origin_edges_are_formula_authority !== false)) ||
      (Number(report.schema_version) >= 36 &&
        (!Number.isSafeInteger(Number(frontier?.window_damage_action_edges)) ||
          Number(frontier.window_damage_action_edges) < 0 ||
          !Number.isSafeInteger(Number(frontier?.window_damage_action_actor_edges)) ||
          Number(frontier.window_damage_action_actor_edges) < 0 ||
          !Number.isSafeInteger(Number(frontier?.window_damage_action_target_edges)) ||
          Number(frontier.window_damage_action_target_edges) < 0 ||
          Number(frontier.window_damage_action_edges) !==
            Number(frontier.window_damage_action_actor_edges) +
              Number(frontier.window_damage_action_target_edges))) ||
      (Number(report.schema_version) >= 37 &&
        Number(frontier?.provider_ownership_proven_effects) !== providerOwnershipProven) ||
      Number(frontier?.provider_rdps_credit_authorized_effects) !== 0 ||
      Number(frontier?.unresolved_evidence_hidden) !== 0 || frontier?.complete !== complete ||
      effects.size !== Number(report.summary?.party_effect_window_frontier_results) ||
      observed.length !== Number(report.summary?.party_effect_window_observed_effects) ||
      Number(report.summary?.party_effect_window_provider_credit_allowed) !==
        [...effects.values()].filter((entry) => entry.provider_rdps_credit_allowed).length ||
      (Number(report.schema_version) >= 37 &&
        Number(report.summary?.party_effect_window_provider_ownership_proven_effects) !==
          providerOwnershipProven) ||
      Number(report.summary?.party_effect_window_remote_cast_rows_synthesized) !== 0 ||
      report.summary?.party_effect_window_frontier_complete !== complete ||
      [...effects.values()].some((entry) =>
        (Number(report.schema_version) < 49 && entry?.affected_entity_role_proven !== false) ||
        entry?.formula_authority !== false ||
        entry?.runtime_authority !== false || entry?.ui_display_authority !== false ||
        entry?.provider_rdps_credit_allowed !== false || !Array.isArray(entry?.blockers) ||
        entry.blockers.length === 0 ||
        !isValidPartyDamageRelationSummary(Number(report.schema_version) >= 32
          ? entry?.affected_entity_damage_actions
          : entry?.affected_entity_as_damage_actor_candidate) ||
        !isValidPartyDamageRelationSummary(Number(report.schema_version) >= 32
          ? entry?.damage_actions_targeting_affected_entity
          : entry?.affected_entity_as_damage_target_candidate) ||
        (Number(report.schema_version) >= 32 &&
          (!isValidPartyIdentityEvidence(entry?.identity_evidence) ||
            !Array.isArray(entry?.reported_duration_millis) ||
            !Array.isArray(entry?.reported_status_levels) ||
            !Array.isArray(entry?.reported_stacks) || !Array.isArray(entry?.reported_counts))) ||
        (Number(report.schema_version) >= 35 && !Array.isArray(entry?.observed_origin_edges)) ||
        (Number(report.schema_version) >= 36 &&
          !isValidPartyDamageActionEdgeSummary(entry?.damage_action_edge_summary)) ||
        (Number(report.schema_version) >= 37 &&
          !isValidPartyEffectProviderOwnershipJoin(
            entry,
            report.inputs?.provider_ownership_proofs ?? [],
          )) ||
        (Number(report.schema_version) >= 38 &&
          !isValidPartyEffectIdentityGate(entry, Number(report.schema_version))) ||
        (Number(report.schema_version) >= 39 &&
          !isValidPartyEffectSeasonContextJoin(
            entry,
            report.inputs?.status_event_season_context_proofs ?? [],
            report.inputs?.season_state_mutation_proof ?? null,
            Number(report.schema_version),
          )) ||
        (Number(report.schema_version) >= 43 &&
          !isValidPartyHasteStackingJoin(
            entry,
            report.inputs?.party_haste_stacking_frontier ?? null,
            report.inputs?.action_speed_formula_proof ?? null,
            Number(report.schema_version),
          )) ||
        (Number(report.schema_version) >= 44 &&
          !isValidPartyHasteActionJoin(
            entry,
            report.inputs?.action_speed_formula_proof ?? null,
            report.inputs?.party_haste_capacity_proof ?? null,
            report.inputs?.action_timing_ancestry_proof ?? null,
            Number(report.schema_version),
          )) ||
        (Number(report.schema_version) >= 45 &&
          !isValidImagineFormulaJoin(
            entry,
            report.inputs?.imagine_formula_proof ?? null,
            report.inputs?.imagine_status_attribute_tier_proof ?? null,
            report.inputs?.imagine_tier_window_counterfactual_inputs ?? null,
            report.inputs?.fatal_spiral_damage_stage_frontier ?? null,
            Number(report.schema_version),
          )) ||
        (Number(report.schema_version) >= 46 &&
          !isValidTargetVulnerabilityFormulaJoin(
            entry,
            report.inputs?.target_vulnerability_formula_proof ?? null,
            Number(report.schema_version),
          )) ||
        (Number(report.schema_version) >= 33 && (() => {
          const receipt = supportEffects.get(String(entry?.effect_id)) ?? null;
          const expectedProven = receipt?.mechanic ===
            "party-haste-percent-status-coefficient" &&
            receipt?.exact_stat_transform_proven === true;
          if (entry?.exact_effect_to_stat_coefficient_proven !== expectedProven) return true;
          if (!expectedProven) return entry?.effect_to_stat_coefficient !== null;
          const coefficient = entry?.effect_to_stat_coefficient;
          return stableStringify(coefficient?.proof) !== stableStringify(receipt.proof) ||
            stableStringify(coefficient?.attribute_ids) !==
              stableStringify(receipt.changed_attribute_ids) ||
            Number(coefficient?.raw_additive_coefficient_units) !==
              Number(receipt.exact_raw_additive_coefficient_units) ||
            stableStringify(coefficient?.exact_origin) !== stableStringify(receipt.exact_origin) ||
            coefficient?.raw_unit_interpretation_authority !== false;
        })())
      )) {
      throw new Error("rDPS proof closure party-effect window frontier is inconsistent");
    }
    for (const entry of observed) {
      const blocker = Number(report.schema_version) >= 49
        ? `party-effect-window-open:${entry.effect_id}:${entry.affected_entity_role_proven
          ? "formula-stacking-rounding-conservation"
          : "affected-entity-scope-membership-formula-stacking-rounding"}`
        : Number(report.schema_version) >= 32
          ? `party-effect-window-open:${entry.effect_id}:affected-entity-scope-membership-formula-stacking-rounding`
        : `party-effect-window-open:${entry.effect_id}:affected-entity-role-formula-stacking-rounding`;
      if (!report.production_readiness?.blockers?.includes(blocker)) {
        throw new Error(`Party-effect window readiness blocker is missing for ${entry.effect_id}`);
      }
    }
    if (Number(report.schema_version) >= 46) {
      const receipt = report.target_vulnerability_formula_frontier ?? null;
      const targetEntry = effects.get("55228") ?? effects.get(55228) ?? null;
      const input = report.inputs?.target_vulnerability_formula_proof ?? null;
      const expectedCount = input === null ? 0 : 1;
      if (Number(report.summary?.target_vulnerability_formula_receipts) !== expectedCount ||
        Number(report.summary?.target_vulnerability_current_build_formulas_proven) !== 0 ||
        Number(report.summary?.target_vulnerability_provider_credit_allowed) !== 0 ||
        (input === null && receipt !== null) ||
        (input !== null && (!targetEntry || receipt === null ||
          stableStringify(receipt) !==
            stableStringify(targetEntry.target_vulnerability_formula) ||
          !report.production_readiness?.blockers?.includes(
            Number(report.schema_version) >= 50
              ? "target-vulnerability-open:55228:exact-current-build-scalar-operation-order-stacking-rounding-conservation"
              : "target-vulnerability-open:55228:exact-current-build-scalar-operation-order-stacking-rounding-protocol-promotion",
          )))) {
        throw new Error("Target-vulnerability formula frontier is inconsistent");
      }
    }
    if (Number(report.schema_version) >= 47) {
      const tierInput = report.inputs?.imagine_status_attribute_tier_proof ?? null;
      const superconductor = effects.get("2110140") ?? effects.get(2110140) ?? null;
      const tierEvidence = superconductor?.imagine_formula?.status_attribute_tier_evidence ?? null;
      const expectedReceipts = tierInput === null ? 0 : 1;
      const expectedExact = tierInput === null ? 0 : 8;
      const expectedUnresolved = tierInput === null ? 0 : 128;
      if (Number(report.summary?.imagine_status_attribute_tier_receipts) !== expectedReceipts ||
        Number(report.summary?.imagine_status_attribute_exact_tier_occurrences) !== expectedExact ||
        Number(report.summary?.imagine_status_attribute_unresolved_applications) !==
          expectedUnresolved ||
        (tierInput === null && tierEvidence !== null) ||
        (tierInput !== null && (!superconductor || tierEvidence === null ||
          stableStringify(tierEvidence.proof) !== stableStringify(tierInput)))) {
        throw new Error("Imagine status-attribute tier frontier is inconsistent");
      }
    }
    if (Number(report.schema_version) >= 48) {
      const input = report.inputs?.imagine_tier_window_counterfactual_inputs ?? null;
      const superconductor = effects.get("2110140") ?? effects.get(2110140) ?? null;
      const receipt = superconductor?.imagine_formula?.tier_window_counterfactual_inputs ?? null;
      const expectedReceipts = input === null ? 0 : 1;
      const expectedWindows = input === null ? 0 : 8;
      const expectedActions = input === null ? 0 : 12557;
      if (Number(report.summary?.imagine_tier_window_counterfactual_input_receipts) !==
          expectedReceipts ||
        Number(report.summary?.imagine_tier_window_exact_windows) !== expectedWindows ||
        Number(report.summary?.imagine_tier_window_complete_inputs) !== expectedWindows ||
        Number(report.summary?.imagine_tier_window_retained_damage_actions) !== expectedActions ||
        Number(report.summary?.imagine_tier_window_counterfactual_damage_deltas) !== 0 ||
        Number(report.summary?.imagine_tier_window_provider_credit_allowed) !== 0 ||
        (input === null && receipt !== null) ||
        (input !== null && (!superconductor || receipt === null ||
          stableStringify(receipt.proof) !== stableStringify(input)))) {
        throw new Error("Imagine tier-window counterfactual-input frontier is inconsistent");
      }
    }
    if (Number(report.schema_version) >= 53) {
      const input = report.inputs?.fatal_spiral_damage_stage_frontier ?? null;
      const fatal = effects.get("2110125") ?? effects.get(2110125) ?? null;
      const receipt = fatal?.imagine_formula?.damage_stage_frontier ?? null;
      const expectedReceipts = input === null ? 0 : 1;
      const expectedMemberships = input === null ? 0 : 27238;
      if (Number(report.summary?.fatal_spiral_damage_stage_frontier_receipts) !==
          expectedReceipts ||
        Number(report.summary?.fatal_spiral_gap_bounded_damage_memberships) !==
          expectedMemberships ||
        (Number(report.schema_version) >= 69 &&
          (Number(report.summary?.fatal_spiral_gap_safe_ownership_resolved_damage_memberships) !==
              Number(receipt?.gap_safe_damage_memberships ?? 0) ||
            Number(report.summary?.fatal_spiral_gap_safe_third_party_provider_memberships) !==
              Number(receipt?.gap_safe_third_party_provider_memberships ?? 0) ||
            Number(report.summary?.fatal_spiral_gap_safe_provider_self_memberships) !==
              Number(receipt?.gap_safe_provider_self_memberships ?? 0) ||
            Number(report.summary?.fatal_spiral_gap_safe_ownership_unresolved_memberships) !==
              Number(receipt?.gap_safe_ownership_unresolved_memberships ?? 0) ||
            (receipt !== null &&
              (receipt.exact_gap_safe_lifecycle_action_membership_join_complete !== true ||
                receipt.provider_ownership_proven_for_all_gap_safe_memberships !== true ||
                Number(receipt.gap_safe_damage_memberships) !== 27238 ||
                Number(receipt.gap_safe_third_party_provider_memberships) !== 25679 ||
                Number(receipt.gap_safe_provider_self_memberships) !== 1559 ||
                Number(receipt.gap_safe_ownership_unresolved_memberships) !== 0)))) ||
        Number(report.summary?.fatal_spiral_controlled_counterfactual_pairs) !== 0 ||
        Number(report.summary?.fatal_spiral_provider_credit_allowed) !== 0 ||
        (Number(report.schema_version) >= 56 &&
          (Number(report.summary
            ?.fatal_spiral_automatic_integer_candidate_evaluator_receipts) !==
              (receipt?.automatic_integer_candidate_evaluator_integrated === true ? 1 : 0) ||
            Number(report.summary
              ?.fatal_spiral_automatic_integer_candidate_evaluated_variants) !==
              Number(receipt?.automatic_integer_candidate_evaluated_variants ?? 0))) ||
        (Number(report.schema_version) >= 57 &&
          (Number(report.summary?.fatal_spiral_retained_capture_exhaustion_receipts) !==
              (receipt
                ?.retained_current_build_present_and_absent_capture_frontier_exhausted === true
                ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_all_reviewed_comparison_samples) !==
              Number(receipt?.all_reviewed_comparison_samples ?? 0))) ||
        (Number(report.schema_version) >= 58 &&
          (Number(report.summary?.fatal_spiral_controlled_capture_client_surface_receipts) !==
              (receipt?.exact_build_hidden_controlled_capture_surface_identified === true
                ? 1 : 0) ||
            Number(report.summary
              ?.fatal_spiral_currently_executable_controlled_capture_routes) !== 0 ||
            (receipt !== null &&
              (receipt.shipping_client_blocks_controlled_capture_submission !== true ||
                receipt.ordinary_production_account_controlled_capture_authorized !== false ||
                receipt.controlled_capture_currently_executable !== false)))) ||
        (Number(report.schema_version) >= 59 &&
          (Number(report.summary?.fatal_spiral_training_scene_access_frontier_receipts) !==
              (receipt?.exact_build_training_scene_access_frontier_reviewed === true
                ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_ordinary_training_scene_entry_routes_proven) !== 0 ||
            (receipt !== null &&
              (receipt.ordinary_training_scene_entry_route_proven !== false ||
                receipt.training_scene_controlled_capture_currently_executable !== false)))) ||
        (Number(report.schema_version) >= 60 &&
          (Number(report.summary?.fatal_spiral_native_immediate_consumer_search_receipts) !==
              (receipt?.exact_native_immediate_family_search_exhausted === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_combat_relevant_exact_immediate_consumers) !== 0 ||
            (receipt !== null &&
              (receipt.combat_relevant_exact_family_immediate_consumer_found !== false ||
                receipt.computed_indirect_table_driven_or_protected_consumer_excluded !== false)))) ||
        (Number(report.schema_version) >= 61 &&
          (Number(report.summary?.fatal_spiral_recovered_partial_prefix_exhaustion_receipts) !==
              (receipt?.retained_recovered_partial_prefix_frontier_exhausted === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_recovered_partial_prefix_comparison_samples) !==
              Number(receipt?.recovered_partial_prefix_comparison_samples ?? 0) ||
            Number(report.summary?.fatal_spiral_recovered_partial_prefix_controlled_pairs) !== 0 ||
            (receipt !== null &&
              (receipt.recovered_partial_prefix_source_capture_integrity_seal_authority !== false ||
                Number(receipt.recovered_partial_prefix_controlled_pairs) !== 0)))) ||
        (Number(report.schema_version) >= 62 &&
          (Number(report.summary?.fatal_spiral_generic_getter_call_search_receipts) !==
              (receipt?.bounded_direct_getter_call_search_exhausted === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_combat_relevant_literal_getter_consumers) !== 0 ||
            Number(report.summary?.fatal_spiral_exact_pointer_slot_reference_search_receipts) !==
              (receipt?.exact_rip_relative_slot_reference_search_exhausted === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_indexed_metadata_consumers_excluded) !== 0 ||
            (receipt !== null &&
              (receipt.combat_relevant_literal_attribute_getter_consumer_found !== false ||
                receipt.indexed_metadata_dispatch_or_protected_consumer_excluded !== false)))) ||
        (Number(report.schema_version) >= 63 &&
          (Number(report.summary?.fatal_spiral_sealed_candidate_readiness_receipts) !==
              (receipt?.recursive_sealed_rlog_candidate_discovery_bounded === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_current_new_sealed_candidate_rlogs) !== 0 ||
            Number(report.summary?.fatal_spiral_source_transition_same_context_pairs) !==
              Number(receipt?.current_source_transition_same_context_pairs ?? 0) ||
            Number(report.summary?.fatal_spiral_source_transition_strict_controlled_pairs) !== 0 ||
            (receipt !== null &&
              (receipt.unseen_seal_positive_control_triggers_refresh !== true ||
                receipt.source_transition_candidate_search_complete !== true ||
                Number(receipt.current_new_sealed_candidate_rlogs) !== 0 ||
                Number(receipt.current_source_transition_same_context_pairs) !== 229 ||
                Number(receipt.current_source_transition_strict_controlled_pairs) !== 0)))) ||
        (Number(report.schema_version) >= 64 &&
          (Number(report.summary?.fatal_spiral_source_transition_minimum_residual_before_configured_endpoint_diagnostic) !==
              Number(receipt?.current_source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic ?? 0) ||
            Number(report.summary?.fatal_spiral_source_transition_minimum_residual_after_configured_endpoint_diagnostic) !==
              Number(receipt?.current_source_transition_minimum_residual_observed_state_dimensions ?? 0) ||
            Number(report.summary?.fatal_spiral_source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions) !== 0 ||
            (receipt !== null &&
              (receipt.configured_endpoint_attribute_family_diagnostic_complete !== true ||
                Number(receipt.current_source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic) !== 14 ||
                Number(receipt.current_source_transition_minimum_residual_observed_state_dimensions) !== 13 ||
                Number(receipt.current_source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions) !== 0)))) ||
        (Number(report.schema_version) >= 65 &&
          (Number(report.summary?.fatal_spiral_configured_endpoint_transition_pairs) !==
              Number(receipt?.current_configured_endpoint_transition_pairs ?? 0) ||
            Number(report.summary?.fatal_spiral_configured_endpoint_transition_residual_ranking_receipts) !==
              (receipt?.configured_endpoint_transition_residual_ranking_complete === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_exact_build_spatial_attribute_identity_receipts) !==
              (receipt?.exact_build_spatial_attribute_identity_proof_complete === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_retained_spatial_raw_value_replay_receipts) !==
              (receipt?.retained_spatial_raw_value_replay_complete === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_spatial_attributes_safe_to_exclude) !== 0 ||
            (receipt !== null &&
              (receipt.configured_endpoint_transition_residual_ranking_complete !== true ||
                receipt.exact_build_spatial_attribute_identity_proof_complete !== true ||
                receipt.retained_spatial_raw_value_replay_complete !== true ||
                receipt.spatial_attributes_safe_to_exclude_from_counterfactual_matching !== false ||
                Number(receipt.current_configured_endpoint_transition_pairs) !== 69 ||
                Number(receipt.current_configured_endpoint_transition_minimum_residual_dimensions) !== 13 ||
                stableStringify(receipt.exact_build_spatial_attribute_identities) !==
                  stableStringify({ 52: "AttrPos", 53: "AttrTargetPos" }) ||
                stableStringify(receipt.spatial_attribute_observations) !==
                  stableStringify({ 52: 132297, 53: 142877 }) ||
                stableStringify(receipt.spatial_attribute_position_decodes) !==
                  stableStringify({ 52: 131558, 53: 142130 }))))) ||
        (Number(report.schema_version) >= 66 &&
          (Number(report.summary?.fatal_spiral_exact_build_action_selector_roster_receipts) !==
              (receipt?.exact_build_action_selector_roster_complete === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_exact_action_selectors) !==
              Number(receipt?.current_exact_action_selectors ?? 0) ||
            Number(report.summary?.fatal_spiral_packet_source_route_matched_transition_pairs) !==
              Number(receipt?.current_packet_source_route_matched_transition_pairs ?? 0) ||
            Number(report.summary?.fatal_spiral_packet_source_route_rejected_transition_pairs) !==
              Number(receipt?.current_packet_source_route_rejected_transition_pairs ?? 0) ||
            (receipt !== null &&
              (receipt.exact_build_action_selector_roster_complete !== true ||
                Number(receipt.packet_source_compatible_static_formula_candidates) !== 6 ||
                receipt.packet_source_mismatch_preserved_unresolved !== true ||
                Number(receipt.current_exact_action_selectors) !== 7 ||
                Number(receipt.current_packet_source_route_matched_transition_pairs) !== 68 ||
                Number(receipt.current_packet_source_route_rejected_transition_pairs) !== 1)))) ||
        (Number(report.schema_version) >= 67 &&
          (Number(report.summary?.fatal_spiral_relative_spatial_relation_audit_receipts) !==
              (receipt?.relative_spatial_relation_audit_complete === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_direct_spatial_relation_complete_transition_pairs) !==
              Number(receipt?.current_direct_spatial_relation_complete_transition_pairs ?? 0) ||
            Number(report.summary?.fatal_spiral_direct_spatial_relation_exact_transition_pairs) !==
              Number(receipt?.current_direct_spatial_relation_exact_transition_pairs ?? 0) ||
            Number(report.summary?.fatal_spiral_direct_spatial_relation_nonexact_transition_pairs) !==
              Number(receipt?.current_direct_spatial_relation_nonexact_transition_pairs ?? 0) ||
            Number(report.summary?.fatal_spiral_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !==
              Number(receipt?.current_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs ?? 0) ||
            (receipt !== null &&
              (receipt.relative_spatial_relation_audit_complete !== true ||
                receipt.direct_source_to_target_geometry_equal_for_all_complete_pairs !== false ||
                Number(receipt.current_direct_spatial_relation_complete_transition_pairs) !== 66 ||
                Number(receipt.current_direct_spatial_relation_exact_transition_pairs) !== 2 ||
                Number(receipt.current_direct_spatial_relation_nonexact_transition_pairs) !== 64 ||
                Number(receipt.current_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12)))) ||
        (Number(report.schema_version) >= 68 &&
          (Number(report.summary?.fatal_spiral_fake_bullet_exact_wire_contract_receipts) !==
              (receipt?.fake_bullet_exact_wire_contract_complete === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_fake_bullet_future_timeline_preservation_receipts) !==
              (receipt?.fake_bullet_future_capture_timeline_preservation_complete === true ? 1 : 0) ||
            Number(report.summary?.fatal_spiral_fake_bullet_current_build_observed_lifecycle_records) !==
              Number(receipt?.fake_bullet_current_build_observed_lifecycle_records ?? 0) ||
            Number(report.summary?.fatal_spiral_fake_bullet_source4_damage_routes_resolved) !== 0 ||
            Number(report.summary?.fatal_spiral_fake_bullet_provider_ownership_proven) !== 0 ||
            (receipt !== null &&
              (receipt.fake_bullet_exact_wire_contract_complete !== true ||
                receipt.fake_bullet_future_capture_timeline_preservation_complete !== true ||
                Number(receipt.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
                receipt.fake_bullet_historical_canonical_logs_backfilled !== false ||
                receipt.fake_bullet_source4_damage_route_resolved !== false ||
                receipt.fake_bullet_provider_ownership_proven !== false)))) ||
        (input === null && receipt !== null) ||
        (input !== null && (!fatal || receipt === null ||
          stableStringify(receipt.proof) !== stableStringify(input)))) {
        throw new Error("Fatal Spiral damage-stage frontier join is inconsistent");
      }
    }
  }
  if (report.schema_version >= 2) verifyProductionReadiness(report);
  const obligations = uniqueIndex(report.obligation_results ?? [], "obligation_id", "closure obligation");
  if (obligations.size !== Number(report.summary?.audited_obligations)) throw new Error("Audited obligation count mismatch");
  if (Number(report.summary?.manifest_indexed_obligations) !== obligations.size) {
    throw new Error("Indexed manifest obligations were not preserved exactly once");
  }
  if (!Number.isSafeInteger(Number(report.summary?.raw_frontier_work_items)) || Number(report.summary.raw_frontier_work_items) < 0) {
    throw new Error("Raw manifest frontier count is invalid");
  }
  if (!Number.isSafeInteger(Number(report.summary?.manifest_explicitly_unindexable_obligations)) ||
    Number(report.summary.manifest_explicitly_unindexable_obligations) < 0) {
    throw new Error("Explicitly unindexable manifest obligation count is invalid");
  }
  if (Number(report.summary?.hidden_omissions) !== 0) throw new Error("Closure report hides obligations");
  const statuses = countBy([...obligations.values()], (entry) => entry.status);
  if (stableStringify(statuses) !== stableStringify(report.summary?.obligation_status_counts ?? {})) throw new Error("Obligation status summary mismatch");
  const sources = uniqueIndex(report.source_results ?? [], "source_rule_id", "source result");
  if (sources.size !== Number(report.summary?.audited_source_rules)) throw new Error("Source result count mismatch");
  const models = uniqueIndex(report.shared_model_results ?? [], "model_key", "shared model result");
  if (models.size !== Number(report.summary?.shared_formula_models)) throw new Error("Shared model count mismatch");
  const proofReceivedRuntimeOpen = [...models.values()].filter((entry) => entry.status === "shared-model-proof-received-runtime-open");
  if (proofReceivedRuntimeOpen.length !== Number(report.summary?.shared_formula_models_proof_received_runtime_open ?? 0)) {
    throw new Error("Proof-received runtime-open model summary mismatch");
  }
  const offlineProvenRuntimeOpen = [...models.values()].filter((entry) => entry.proof_states?.includes(OFFLINE_FORMULA_PROOF_STATE));
  if (offlineProvenRuntimeOpen.length !== Number(report.summary?.shared_formula_models_offline_proven_runtime_open ?? 0)) {
    throw new Error("Offline-proven runtime-open model summary mismatch");
  }
  const routeProvenRuntimeOpen = [...models.values()].filter((entry) => entry.proof_states?.includes(CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE));
  if (routeProvenRuntimeOpen.length !== Number(report.summary?.shared_formula_models_canonical_runtime_input_route_proven_runtime_open ?? 0)) {
    throw new Error("Canonical-runtime-route-proven runtime-open model summary mismatch");
  }
  const registryOnlyModels = [...models.values()].filter((entry) => entry.registry_only_proof_route === true);
  if (registryOnlyModels.length !== Number(report.summary?.registry_only_proof_route_models ?? 0)) {
    throw new Error("Registry-only proof-route model summary mismatch");
  }
  for (const model of proofReceivedRuntimeOpen) {
    if (!Array.isArray(model.proof_receipt) || model.proof_receipt.length === 0 ||
      model.blockers.includes("shared-static-model-has-no-current-build-proof-receipt")) {
      throw new Error(`Shared proof receipt was not preserved safely for ${model.model_key}`);
    }
    if (!Array.isArray(model.proof_states) || model.proof_states.length === 0 ||
      model.proof_states.some((state) => !ALLOWED_SHARED_PROOF_STATES.has(state))) {
      throw new Error(`Shared proof state was not preserved safely for ${model.model_key}`);
    }
    const expectedOpenGates = uniqueSorted(
      model.proof_receipt.flatMap((receipt) => receipt.still_required_runtime_gates ?? []),
    );
    if (stableStringify(model.still_required_runtime_gates ?? []) !== stableStringify(expectedOpenGates) ||
      expectedOpenGates.some((gate) => !model.blockers.includes(`shared-proof-runtime-gate-open:${gate}`))) {
      throw new Error(`Shared proof runtime gates were not preserved safely for ${model.model_key}`);
    }
  }
  for (const model of registryOnlyModels) {
    if (model.source_count !== 0 || model.static_blocker_obligations !== 0 || model.runtime_manifest_obligations !== 0 ||
      model.source_rule_ids.length !== 0 || model.workbench_obligation_ids.length !== 0 || model.runtime_obligation_ids.length !== 0) {
      throw new Error(`Registry-only proof route ${model.model_key} fabricated downstream obligations`);
    }
  }
  for (const entry of obligations.values()) {
    if (!Array.isArray(entry.blockers) || !entry.gates || !entry.evidence) throw new Error(`Incomplete closure result ${entry.obligation_id}`);
    if (Number(report.schema_version) >= 79 && [
      "external-target-state-counterfactual",
      "external-recipient-counterfactual",
    ].includes(entry.transfer_gate?.kind) && entry.gates.formula_inputs === true &&
      (Number(entry.evidence.formula_input_snapshots) <= 0 ||
        Number(entry.evidence.complete_formula_input_snapshots) !==
          Number(entry.evidence.formula_input_snapshots))) {
      throw new Error(`Counterfactual formula inputs were closed without a complete snapshot set for ${entry.obligation_id}`);
    }
    if (!["exact-id", "runtime-selector-rekey", "manifest-new-no-observation"].includes(entry.correlation_match?.kind)) {
      throw new Error(`Incomplete aggregate correlation provenance ${entry.obligation_id}`);
    }
    if (entry.status === "proven-promotable" && (!Object.values(entry.gates).every(Boolean) || entry.blockers.length !== 0)) {
      throw new Error(`Unsafe promotion ${entry.obligation_id}`);
    }
    const componentScalarOwnership = entry.evidence.component_static_scalar_provider_ownership;
    const hasComponentScalarGapInventory = componentScalarOwnership?.gap_groups !== null &&
      componentScalarOwnership?.gap_groups !== undefined;
    const exactComponentScalarOwnership =
      componentScalarOwnership?.exact_provider_ownership_proven === true;
    if (componentScalarOwnership &&
      (!Number.isSafeInteger(Number(componentScalarOwnership.unresolved_provider_status_events)) ||
        (exactComponentScalarOwnership
          ? Number(componentScalarOwnership.unresolved_provider_status_events) !== 0
          : Number(componentScalarOwnership.unresolved_provider_status_events) <= 0) ||
        (hasComponentScalarGapInventory &&
          (!Number.isSafeInteger(Number(componentScalarOwnership.gap_groups)) ||
            (exactComponentScalarOwnership
              ? Number(componentScalarOwnership.gap_groups) !== 0
              : Number(componentScalarOwnership.gap_groups) <= 0) ||
            Number(componentScalarOwnership
              .unresolved_events_with_same_source_separate_stable_player_resolution) +
              Number(componentScalarOwnership
                .unresolved_events_without_same_source_stable_player_resolution) !==
                Number(componentScalarOwnership.unresolved_provider_status_events))) ||
        entry.gates.exact_component_scalar_provider_ownership !== exactComponentScalarOwnership ||
        (exactComponentScalarOwnership
          ? (entry.status === "observed-external-scope-awaiting-provider-ownership" ||
            entry.blockers.includes("component-static-scalar-provider-ownership-unproven"))
          : (entry.status !== "observed-external-scope-awaiting-provider-ownership" ||
            !entry.blockers.includes("component-static-scalar-provider-ownership-unproven"))))) {
      throw new Error(`Unsafe component scalar provider-ownership gate ${entry.obligation_id}`);
    }
  }
  const runtimeEffects = uniqueIndex(
    report.packet_observed_runtime_effect_results ?? [],
    "effect_id",
    "packet-observed runtime effect",
  );
  if (runtimeEffects.size !== Number(report.summary?.packet_observed_runtime_effects ?? 0)) {
    throw new Error("Packet-observed runtime effect count mismatch");
  }
  const runtimeStatuses = countBy([...runtimeEffects.values()], (entry) => entry.status);
  if (stableStringify(runtimeStatuses) !== stableStringify(report.summary?.packet_observed_runtime_status_counts ?? {})) {
    throw new Error("Packet-observed runtime effect status summary mismatch");
  }
  const externalRuntimeEffects = [...runtimeEffects.values()].filter((entry) => EXTERNAL_RUNTIME_EFFECT_STATUSES.has(entry.status));
  if (externalRuntimeEffects.length !== Number(report.summary?.packet_observed_runtime_external_candidates ?? 0)) {
    throw new Error("Packet-observed external runtime effect summary mismatch");
  }
  const nonOutgoingRuntimeEffects = [...runtimeEffects.values()].filter((entry) => entry.status === "packet-observed-non-outgoing-context");
  if (nonOutgoingRuntimeEffects.length !== Number(report.summary?.packet_observed_runtime_non_outgoing_context ?? 0)) {
    throw new Error("Packet-observed non-outgoing runtime effect summary mismatch");
  }
  for (const entry of runtimeEffects.values()) {
    if (!entry.gates || !entry.evidence || !Array.isArray(entry.blockers)) {
      throw new Error(`Incomplete packet-observed runtime effect ${entry.effect_id}`);
    }
    if (entry.status === "runtime-model-ready-awaiting-strict-conservation" &&
      (!entry.evidence.runtime_calculation_ready || entry.gates.strict_counterfactual_conservation ||
        !entry.blockers.includes("strict-counterfactual-projection-and-conservation-unproven"))) {
      throw new Error(`Unsafe runtime model-ready status ${entry.effect_id}`);
    }
    if (entry.status === "runtime-attribution-promoted-exact-subset") {
      const exactReplayDelta = entry.evidence.exact_replay_attributed_bonus_damage_rational;
      if (entry.resolution_scope !== "current-build-conserved-unambiguous-event-subset" ||
        entry.full_effect_family_resolved !== false ||
        entry.blockers.length !== 0 || Object.values(entry.gates).some((value) => value !== true) ||
        entry.evidence.runtime_calculation_ready !== true ||
        !Number.isSafeInteger(Number(entry.evidence.exact_replay_damage_events)) ||
        Number(entry.evidence.exact_replay_damage_events) <= 0 ||
        !exactReplayDelta ||
        !positiveRational(normalizeRational(
          BigInt(String(exactReplayDelta.numerator ?? "0")),
          BigInt(String(exactReplayDelta.denominator ?? "0")),
        )) ||
        !Array.isArray(entry.evidence.exact_replay_affected_damage_ids) ||
        entry.evidence.exact_replay_affected_damage_ids.length === 0 ||
        entry.evidence.ambiguous_provider_windows_remain_deferred !== true ||
        !Number.isSafeInteger(Number(entry.evidence.ambiguous_provider_window_events)) ||
        Number(entry.evidence.ambiguous_provider_window_events) < 0) {
        throw new Error(`Unsafe exact-subset runtime attribution promotion ${entry.effect_id}`);
      }
    }
    if (entry.status === "packet-observed-non-outgoing-context" &&
      (entry.blockers.length !== 0 ||
        entry.evidence.component_routing?.proven_no_outgoing_attribution !== true ||
        entry.evidence.component_routing?.runtime_credit_candidate !== false)) {
      throw new Error(`Unsafe non-outgoing runtime effect classification ${entry.effect_id}`);
    }
  }
  const counterfactualResults = report.counterfactual_frontier_results ?? [];
  const counterfactualKeys = new Set();
  for (const entry of counterfactualResults) {
    const key = `${String(entry.locus)}:${String(entry.effect_id)}`;
    if (counterfactualKeys.has(key)) throw new Error(`Duplicate closure counterfactual locus ${key}`);
    counterfactualKeys.add(key);
    if (!["source", "target"].includes(entry.locus) ||
      entry.formula_authority !== false || entry.runtime_authority !== false ||
      !Array.isArray(entry.blockers) || entry.blockers.length === 0 ||
      !entry.blockers.includes("counterfactual-frontier-declares-no-formula-authority") ||
      !entry.blockers.includes("counterfactual-frontier-declares-no-runtime-authority") ||
      ![
        "controlled-delta-observed-proof-open",
        "controlled-equal-output-observed-proof-open",
        "no-controlled-counterfactual-pair",
      ].includes(entry.status)) {
      throw new Error(`Unsafe closure counterfactual frontier result ${key}`);
    }
    const exactCounterfactual = entry.exact_recorded_inputs ?? {};
    validateBladeSweepCandidateProjection(
      exactCounterfactual.blade_sweep_candidate_projection,
      Number(entry.effect_id),
      String(entry.locus),
      Object.prototype.hasOwnProperty.call(
        exactCounterfactual,
        "blade_sweep_candidate_projection",
      ) ? 5 : 4,
    );
    const nearSourceAudit = entry.near_controlled_source_attribute_diagnostic;
    if (nearSourceAudit) {
      validateNearSourceCounterfactualAudit(
        nearSourceAudit,
        Number(entry.effect_id),
        String(entry.locus),
      );
      if (Number(nearSourceAudit.candidate_absent_near_pairs) === 0 &&
        !entry.blockers.includes("near-source-controlled-counterfactual-pair-missing")) {
        throw new Error(`Closure counterfactual ${key} omitted its near-source blocker`);
      }
    }
    if (Number(entry.counterfactual_frontier_schema_version) >= 8) {
      const processing = entry.counterfactual_frontier_processing;
      if (processing?.measured_peak_within_configured_limit !== true ||
        !Number.isSafeInteger(Number(processing?.memory_limit_mib)) ||
        Number(processing.memory_limit_mib) <= 0 ||
        !Number.isSafeInteger(Number(processing?.measured_peak_working_set_bytes)) ||
        Number(processing.measured_peak_working_set_bytes) <= 0 ||
        Number(processing.measured_peak_working_set_bytes) >
          Number(processing.memory_limit_mib) * 1024 * 1024 ||
        !Array.isArray(processing?.selected_effect_ids) ||
        !processing.selected_effect_ids.map(Number).includes(Number(entry.effect_id))) {
        throw new Error(`Closure counterfactual ${key} has unsafe bounded processing evidence`);
      }
    }
    if (Number(entry.counterfactual_frontier_schema_version) >= 9) {
      const baselineProof = entry.cross_entity_baseline_proof;
      const diagnostics = entry.cross_entity_diagnostics;
      const selectedAttributeIds = entry.counterfactual_frontier_processing
        ?.selected_source_transition_attribute_ids?.map(Number);
      if (Number(baselineProof?.schema_version) !== 8 ||
        !String(baselineProof?.path ?? "") ||
        !Number.isSafeInteger(Number(baselineProof?.bytes)) || Number(baselineProof.bytes) <= 0 ||
        !/^sha256:[0-9a-f]{64}$/.test(String(baselineProof?.sha256 ?? "")) ||
        entry.structurally_absent_remote_skill_cast_packets_required !== false ||
        !diagnostics || typeof diagnostics !== "object" ||
        !Array.isArray(selectedAttributeIds) || selectedAttributeIds.length === 0) {
        throw new Error(`Closure counterfactual ${key} has unsafe cross-entity provenance`);
      }
      const schemaVersion = Number(entry.counterfactual_frontier_schema_version);
      const diagnosticSpecifications = [
        [9, "cross_entity_formula_state_diagnostic", "formula-state"],
        [10, "cross_entity_source_transition_diagnostic", "source-transition"],
        [11, "cross_entity_source_transition_target_current_hp_excluded_diagnostic", "source-transition"],
        [12,
          "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic",
          "source-transition"],
        [13,
          "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic",
          "source-transition"],
      ];
      for (const [minimumSchema, field, kind] of diagnosticSpecifications) {
        if (schemaVersion < minimumSchema) continue;
        const row = diagnostics[field];
        if (row == null) continue;
        if (Number(row.effect_id) !== Number(entry.effect_id) || row.locus !== entry.locus ||
          row.formula_authority !== false || row.runtime_authority !== false ||
          row.ui_display_authority !== false || row.provider_rdps_credit_allowed !== false) {
          throw new Error(`Closure counterfactual ${key} has unsafe ${field}`);
        }
        if (kind === "formula-state") {
          validateCrossEntityFormulaStateRow(row, Number(entry.effect_id));
        } else {
          validateCrossEntitySourceTransitionRow(
            row,
            Number(entry.effect_id),
            selectedAttributeIds,
            schemaVersion,
          );
        }
      }
    }
    const ownership = entry.provider_ownership_evidence;
    if (ownership) {
      if (ownership.effect_id !== String(entry.effect_id) ||
        ownership.run_scoped_player_ownership_proven !== true ||
        ownership.formula_authority !== false || ownership.runtime_authority !== false ||
        ownership.provider_rdps_credit_allowed !== false ||
        !Number.isSafeInteger(Number(ownership.selected_status_events)) ||
        Number(ownership.selected_status_events) <= 0 ||
        (ownership.stable_player_character_id_proven_for_every_status_event === true &&
          (!Array.isArray(ownership.stable_player_character_ids) ||
            ownership.stable_player_character_ids.length === 0 ||
            ownership.stable_player_character_ids.some((characterId) => !/^\d+$/.test(characterId)))) ||
        entry.blockers.includes("provider entity ownership to a player is unproven") ||
        (ownership.stable_player_character_id_proven_for_every_status_event !== true &&
          !entry.blockers.includes("stable-player-character-id-unproven-for-cross-run-join"))) {
        throw new Error(`Unsafe provider ownership evidence for counterfactual frontier result ${key}`);
      }
    }
    const transform = entry.integer_transform_constraints;
    if (transform && (transform.effect_id !== String(entry.effect_id) ||
      transform.locus !== entry.locus || transform.exact_transform_proven !== false ||
      transform.exact_static_status_transform_binding_proven !== false ||
      transform.formula_authority !== false || transform.runtime_authority !== false ||
      transform.provider_rdps_credit_allowed !== false ||
      !Number.isSafeInteger(Number(transform.exact_divergent_controlled_examples)) ||
      Number(transform.exact_divergent_controlled_examples) <= 0 ||
      !entry.blockers.includes("exact transform, stacking, operation order, and integer rounding are unproven"))) {
      throw new Error(`Unsafe integer transform constraints for counterfactual frontier result ${key}`);
    }
    const eventLocalConservation = transform?.event_local_counterfactual_conservation;
    if (eventLocalConservation &&
      (eventLocalConservation.scope !== "single_exact_controlled_observed_damage_event_pairs" ||
        Number(eventLocalConservation.summary?.event_pairs) <= 0 ||
        eventLocalConservation.summary?.arithmetic_conservation_holds_for_every_pair !== true ||
        eventLocalConservation.summary?.exact_recorded_inputs_controlled_for_every_pair !== true ||
        eventLocalConservation.summary?.provider_player_identity_proven_for_every_pair !== true ||
        eventLocalConservation.interpretation
          ?.event_local_counterfactual_arithmetic_conservation_proven !== true ||
        eventLocalConservation.interpretation
          ?.observed_controlled_delta_is_not_a_general_formula !== true ||
        eventLocalConservation.interpretation?.causal_provider_contribution_proven !== false ||
        eventLocalConservation.interpretation?.exact_transform_proven !== false ||
        eventLocalConservation.interpretation?.formula_stage_and_operation_order_proven !== false ||
        eventLocalConservation.interpretation?.runtime_integer_rounding_proven !== false ||
        eventLocalConservation.interpretation?.canonical_party_replay_conservation_proven !== false ||
        eventLocalConservation.interpretation?.formula_authority !== false ||
        eventLocalConservation.interpretation?.runtime_authority !== false ||
        eventLocalConservation.interpretation?.ui_authority !== false ||
        eventLocalConservation.interpretation?.provider_rdps_credit_allowed !== false)) {
      throw new Error(`Unsafe event-local conservation evidence for counterfactual frontier result ${key}`);
    }
    const nearControlledExhaustion = transform?.near_controlled_exhaustion;
    if (nearControlledExhaustion &&
      (Number(nearControlledExhaustion.summary?.matching_capture_runs) <= 0 ||
        Number(nearControlledExhaustion.summary?.samples) <= 0 ||
        nearControlledExhaustion.summary?.exact_divergent_capture_runs !== 1 ||
        nearControlledExhaustion.summary?.near_controlled_target_divergent_pairs !== 0 ||
        nearControlledExhaustion.summary?.equal_output_status_bundle_examples !== 1 ||
        nearControlledExhaustion.interpretation
          ?.independent_divergent_baseline_replication_proven !== false ||
        nearControlledExhaustion.interpretation
          ?.equal_output_status_bundle_is_an_isolated_effect_zero_proof !== false ||
        nearControlledExhaustion.interpretation?.exact_transform_proven !== false ||
        nearControlledExhaustion.interpretation?.formula_authority !== false ||
        nearControlledExhaustion.interpretation?.runtime_authority !== false ||
        nearControlledExhaustion.interpretation?.ui_authority !== false ||
        nearControlledExhaustion.interpretation?.provider_rdps_credit_allowed !== false)) {
      throw new Error(`Unsafe near-controlled exhaustion evidence for counterfactual frontier result ${key}`);
    }
    const providerFormulaContext = transform?.provider_formula_context_summary;
    if (providerFormulaContext &&
      (providerFormulaContext.formula_context_embedded_for_every_example !== true ||
        providerFormulaContext.exact_provider_attribute_state_controlled_for_every_example !== true ||
        providerFormulaContext.provider_attribute_state_observed_for_every_example !== true ||
        providerFormulaContext.provider_formula_base_input_proven !== false ||
        !Array.isArray(providerFormulaContext.missing_or_unproven_inputs) ||
        providerFormulaContext.missing_or_unproven_inputs.length === 0 ||
        (report.policy?.provider_formula_context_gaps_are_explicit_blockers === true &&
          (!entry.blockers.includes("provider-at-event formula base input is unproven") ||
            providerFormulaContext.missing_or_unproven_inputs.some((blocker) =>
              !entry.blockers.includes(`provider formula context: ${String(blocker)}`)
            ))))) {
      throw new Error(`Unsafe provider formula context for counterfactual frontier result ${key}`);
    }
    const providerFormulaInputCoverage = transform?.provider_formula_input_coverage;
    if (providerFormulaInputCoverage &&
      (providerFormulaInputCoverage.exact_provider_and_input_identity_match !== true ||
        providerFormulaInputCoverage.interpretation?.packet_absence_is_not_zero !== true ||
        providerFormulaInputCoverage.interpretation?.unobserved_inputs_are_not_backfilled_or_derived !== true ||
        providerFormulaInputCoverage.interpretation?.spheal_operator_contract_proven !== false ||
        providerFormulaInputCoverage.interpretation?.effect_output_to_status_transform_binding_proven !== false ||
        providerFormulaInputCoverage.interpretation?.provider_formula_base_input_proven !== false ||
        providerFormulaInputCoverage.interpretation?.formula_authority !== false ||
        providerFormulaInputCoverage.interpretation?.runtime_authority !== false ||
        providerFormulaInputCoverage.interpretation?.provider_rdps_credit_allowed !== false ||
        !Array.isArray(providerFormulaInputCoverage.proven_provider_entity_uuids) ||
        providerFormulaInputCoverage.proven_provider_entity_uuids.length === 0 ||
        !Array.isArray(providerFormulaInputCoverage.capture_inputs) ||
        providerFormulaInputCoverage.capture_inputs.length !==
          Number(providerFormulaContext?.matching_capture_count ?? -1) ||
        providerFormulaInputCoverage.proven_provider_entity_uuids.length !==
          Number(providerFormulaContext?.proven_effect_provider_count ?? -1) ||
        Number(providerFormulaInputCoverage.audited_candidate_formula_inputs
          ?.physical_attack?.observation_count) !==
          Number(providerFormulaContext?.matching_capture_physical_attack_observation_count))) {
      throw new Error(`Unsafe provider formula-input coverage for counterfactual frontier result ${key}`);
    }
    const spHealOperatorEvidence = transform?.spheal_operator_evidence;
    if (spHealOperatorEvidence &&
      (spHealOperatorEvidence.summary?.exact_effect_output_packet_observed !== false ||
        spHealOperatorEvidence.summary?.exact_effect_occurrence_proof_selected_events !== 0 ||
        spHealOperatorEvidence.summary?.spheal_family_wide_single_hp_ratio_proven !== false ||
        spHealOperatorEvidence.summary?.exact_effect_spheal_coefficient_to_hp_basis_binding_proven !== false ||
        spHealOperatorEvidence.summary?.damage_script_identity_alone_proves_operator !== false ||
        spHealOperatorEvidence.summary?.exact_effect_operator_proven !== false ||
        spHealOperatorEvidence.interpretation?.exact_effect_output_occurrence_missing !== true ||
        spHealOperatorEvidence.interpretation
          ?.exact_effect_output_absent_in_all_complete_matching_build_capture_inputs !== true ||
        spHealOperatorEvidence.interpretation?.heterogeneous_spheal_family_evidence !== true ||
        spHealOperatorEvidence.interpretation?.family_name_transfer_to_exact_effect_allowed !== false ||
        spHealOperatorEvidence.interpretation?.exact_effect_formula_authority !== false ||
        spHealOperatorEvidence.interpretation?.exact_effect_runtime_authority !== false ||
        spHealOperatorEvidence.interpretation?.provider_rdps_credit_allowed !== false ||
        spHealOperatorEvidence.formula_authority !== false ||
        spHealOperatorEvidence.runtime_authority !== false ||
        spHealOperatorEvidence.provider_rdps_credit_allowed !== false ||
        !Array.isArray(spHealOperatorEvidence.input_rlogs) ||
        spHealOperatorEvidence.input_rlogs.length !==
          Number(providerFormulaContext?.matching_capture_count ?? -1) ||
        !Array.isArray(spHealOperatorEvidence.exact_effect_occurrence_rlogs) ||
        spHealOperatorEvidence.exact_effect_occurrence_rlogs.length !==
          Number(providerFormulaContext?.exact_effect_spheal_occurrence_proof_capture_count ?? -1) ||
        Number(spHealOperatorEvidence.summary?.exact_effect_occurrence_proof_healing_events_scanned) !==
          Number(providerFormulaContext
            ?.exact_effect_spheal_occurrence_proof_healing_events_scanned ?? -1))) {
      throw new Error(`Unsafe SpHeal operator evidence for counterfactual frontier result ${key}`);
    }
    const staticFormulaInputs = transform?.static_formula_input_candidates;
    if (staticFormulaInputs &&
      (staticFormulaInputs.interpretation?.exact_static_status_transform_binding_proven !== false ||
        staticFormulaInputs.interpretation?.formula_authority !== false ||
        staticFormulaInputs.interpretation?.runtime_authority !== false ||
        !Array.isArray(staticFormulaInputs.rows) || staticFormulaInputs.rows.length === 0 ||
        staticFormulaInputs.rows.some((row) => Number(row.type_enum) !== Number(entry.effect_id)))) {
      throw new Error(`Unsafe static formula inputs for counterfactual frontier result ${key}`);
    }
    const componentScalar = entry.component_static_scalar_evidence;
    const exactComponentProviderOwnership =
      componentScalar?.exact_provider_ownership_proven === true;
    const hasSameAxisStatusEvidence =
      componentScalar?.target_mitigation_near_pair_candidate?.same_axis_status_invariance !== undefined;
    const hasCounterfactualDiscriminants =
      componentScalar?.counterfactual_discriminants !== null &&
      componentScalar?.counterfactual_discriminants !== undefined;
    const hasTargetStatusActionRouteAudit =
      componentScalar?.target_status_action_route_audit !== null &&
      componentScalar?.target_status_action_route_audit !== undefined;
    const hasTargetDefensePercentLifecycleProof =
      componentScalar?.target_defense_percent_lifecycle_proof !== null &&
      componentScalar?.target_defense_percent_lifecycle_proof !== undefined;
    const hasRawPercentLifecycleProof = Number(componentScalar?.proof_schema_version) >= 13;
    const hasFightAttributeScopeProof = Number(componentScalar?.proof_schema_version) >= 14;
    const hasCritCoTransitionProof = Number(componentScalar?.proof_schema_version) >= 15;
    const hasTargetDefenseStatusDiagnosticRollup =
      Number(componentScalar?.proof_schema_version) >= 16;
    const hasTargetMitigationActorSceneExhaustion =
      Number(componentScalar?.proof_schema_version) >= 17;
    const hasRlogGapWindowAudit = Number(componentScalar?.proof_schema_version) >= 18;
    const hasRlogTransitionCounterfactualAudit =
      Number(componentScalar?.proof_schema_version) >= 19;
    const hasRlogTransitionMismatchFrontier =
      Number(componentScalar?.proof_schema_version) >= 20;
    const hasRlogOpaqueAttributeAudit =
      Number(componentScalar?.proof_schema_version) >= 21;
    const hasSourceStatusConfounderRouteAudit =
      Number(componentScalar?.proof_schema_version) >= 22;
    const hasSourceStatusLocalObservableAudit =
      Number(componentScalar?.proof_schema_version) >= 23;
    const hasTargetEffectFormulaProof =
      Number(componentScalar?.proof_schema_version) >= 24;
    const hasRlogTransitionStagedResidualFrontier =
      Number(componentScalar?.proof_schema_version) >= 27;
    const hasLuckyPacketComponentProof =
      Number(componentScalar?.proof_schema_version) >= 28;
    const hasMAttackLuckyMitigationDiagnostic =
      Number(componentScalar?.proof_schema_version) >= 29;
    const hasAttackLuckyMitigationDiagnostic =
      Number(componentScalar?.proof_schema_version) >= 30;
    const hasLuckyParentMultiplierProof =
      Number(componentScalar?.proof_schema_version) >= 31;
    const hasGroupedLuckyRelationProof =
      Number(componentScalar?.proof_schema_version) >= 32;
    const hasExhaustiveLocalSourceAttributeProof =
      Number(componentScalar?.proof_schema_version) >= 33;
    const hasTargetDefenseTransformBoundary =
      Number(componentScalar?.proof_schema_version) >= 34;
    if (componentScalar &&
      (entry.locus !== "target" || componentScalar.effect_id !== String(entry.effect_id) ||
        componentScalar.exact_static_scalar_proven !== true ||
        Number(componentScalar.observed_runtime_tier) !== 5 ||
        Number(componentScalar.observed_runtime_armor_penetration_basis_points) !== 650 ||
        Number(componentScalar.observed_runtime_armor_penetration_percent) !== 6.5 ||
        !Number.isSafeInteger(Number(componentScalar.unresolved_provider_status_events)) ||
        (exactComponentProviderOwnership
          ? Number(componentScalar.unresolved_provider_status_events) !== 0
          : Number(componentScalar.unresolved_provider_status_events) <= 0) ||
        (componentScalar.provider_ownership_gap_worklist &&
          ((exactComponentProviderOwnership
            ? componentScalar.provider_ownership_gap_worklist.status !==
              "exact-provider-ownership-proven"
            : componentScalar.provider_ownership_gap_worklist.status !==
              "exact-gap-inventory-acquisition-required") ||
            Number(componentScalar.provider_ownership_gap_worklist.unresolved_status_events) !==
              Number(componentScalar.unresolved_provider_status_events) ||
            (exactComponentProviderOwnership
              ? Number(componentScalar.provider_ownership_gap_worklist.gap_groups) !== 0
              : Number(componentScalar.provider_ownership_gap_worklist.gap_groups) <= 0) ||
            componentScalar.provider_ownership_gap_worklist.exact_provider_ownership_proven !==
              exactComponentProviderOwnership ||
            componentScalar.provider_ownership_gap_worklist.formula_authority !== false ||
            componentScalar.provider_ownership_gap_worklist.runtime_authority !== false ||
            componentScalar.provider_ownership_gap_worklist.provider_rdps_credit_allowed !== false)) ||
        (Number(componentScalar.prior_status_instance_player_owned_status_events ?? 0) > 0 &&
          Number(componentScalar.provider_ownership_gap_worklist
            ?.prior_status_instance_player_owned_status_events) !==
            Number(componentScalar.prior_status_instance_player_owned_status_events)) ||
        (Number(componentScalar.same_wire_packet_player_owned_status_events ?? 0) > 0 &&
          (Number(componentScalar.provider_ownership_gap_worklist
            ?.same_wire_packet_player_owned_status_events) !==
            Number(componentScalar.same_wire_packet_player_owned_status_events) ||
            exactComponentProviderOwnership !== true)) ||
        componentScalar.exact_armor_to_damage_equation_proven !== false ||
        componentScalar.exact_damage_projection_proven !== false ||
        componentScalar.exact_operation_order_proven !== false ||
        componentScalar.exact_integer_rounding_proven !== false ||
        componentScalar.packet_conservation_proven !== false ||
        componentScalar.formula_authority !== false || componentScalar.runtime_authority !== false ||
        componentScalar.provider_rdps_credit_allowed !== false ||
        componentScalar.target_mitigation_evidence?.status !==
          "no-controlled-target-mitigation-pairs" ||
        Number(componentScalar.target_mitigation_evidence?.damage_samples) <= 0 ||
        Number(componentScalar.target_mitigation_evidence?.audited_axis_samples) <= 0 ||
        Number(componentScalar.target_mitigation_evidence?.controlled_groups) !== 0 ||
        componentScalar.target_mitigation_evidence?.exact_target_mitigation_formula_proven !== false ||
        componentScalar.target_mitigation_evidence?.operation_order_and_integer_rounding_proven !== false ||
        componentScalar.target_mitigation_evidence?.packet_conservation_proven !== false ||
        componentScalar.target_mitigation_evidence?.formula_authority !== false ||
        componentScalar.target_mitigation_evidence?.runtime_authority !== false ||
        componentScalar.target_mitigation_evidence?.provider_rdps_credit_allowed !== false ||
        componentScalar.global_target_mitigation_evidence?.status !==
          "no-controlled-target-mitigation-pairs" ||
        Number(componentScalar.global_target_mitigation_evidence?.matching_build_source_rlogs) <= 0 ||
        Number(componentScalar.global_target_mitigation_evidence?.damage_samples) <
          Number(componentScalar.target_mitigation_evidence?.damage_samples) ||
        Number(componentScalar.global_target_mitigation_evidence?.audited_axis_samples) <
          Number(componentScalar.target_mitigation_evidence?.audited_axis_samples) ||
        Number(componentScalar.global_target_mitigation_evidence?.controlled_groups) !== 0 ||
        componentScalar.global_target_mitigation_evidence
          ?.exact_target_mitigation_formula_proven !== false ||
        componentScalar.global_target_mitigation_evidence
          ?.operation_order_and_integer_rounding_proven !== false ||
        componentScalar.global_target_mitigation_evidence?.packet_conservation_proven !== false ||
        componentScalar.global_target_mitigation_evidence?.formula_authority !== false ||
        componentScalar.global_target_mitigation_evidence?.runtime_authority !== false ||
        componentScalar.global_target_mitigation_evidence?.provider_rdps_credit_allowed !== false ||
        (componentScalar.target_mitigation_offline_exhaustion &&
          (componentScalar.target_mitigation_offline_exhaustion.status !==
            "exact-current-build-aggregate-offline-client-and-packet-search-exhausted-final-validation-required" ||
            Number(componentScalar.target_mitigation_offline_exhaustion.packet_capture_proofs) <= 0 ||
            Number(componentScalar.target_mitigation_offline_exhaustion.packet_source_rlogs) <
              Number(componentScalar.target_mitigation_offline_exhaustion.packet_capture_proofs) ||
            Number(componentScalar.target_mitigation_offline_exhaustion.packet_damage_samples) <
              Number(componentScalar.global_target_mitigation_evidence.damage_samples) ||
            Number(componentScalar.target_mitigation_offline_exhaustion
              .packet_audited_axis_samples) <
              Number(componentScalar.global_target_mitigation_evidence.audited_axis_samples) ||
            Number(componentScalar.target_mitigation_offline_exhaustion
              .packet_samples_with_physical_or_refined_defense) <= 0 ||
            Number(componentScalar.target_mitigation_offline_exhaustion
              .packet_samples_with_magic_defense) < 0 ||
            Number(componentScalar.target_mitigation_offline_exhaustion
              .controlled_counterfactual_pairs) !== 0 ||
            Number(componentScalar.target_mitigation_offline_exhaustion
              .promoted_combat_formulas) !== 0 ||
            !Array.isArray(componentScalar.target_mitigation_offline_exhaustion.final_validation) ||
            componentScalar.target_mitigation_offline_exhaustion.final_validation.length !== 2 ||
            componentScalar.target_mitigation_offline_exhaustion
              .exact_target_mitigation_formula_proven !== false ||
            componentScalar.target_mitigation_offline_exhaustion
              .operation_order_and_integer_rounding_proven !== false ||
            componentScalar.target_mitigation_offline_exhaustion.packet_conservation_proven !== false ||
            componentScalar.target_mitigation_offline_exhaustion.formula_authority !== false ||
            componentScalar.target_mitigation_offline_exhaustion.runtime_authority !== false ||
            componentScalar.target_mitigation_offline_exhaustion
              .provider_rdps_credit_allowed !== false)) ||
        (componentScalar.target_mitigation_acquisition_worklist &&
          (componentScalar.target_mitigation_acquisition_worklist.status !==
            (hasSameAxisStatusEvidence
              ? "acquisition-required-strict-controls-status-damage-relevance-observed"
              : "acquisition-required-no-target-status-only-near-pair") ||
            Number(componentScalar.target_mitigation_acquisition_worklist
              .matching_build_capture_diagnostics) <= 0 ||
            Number(componentScalar.target_mitigation_acquisition_worklist.damage_samples) !==
              Number(componentScalar.target_mitigation_evidence.damage_samples) ||
            Number(componentScalar.target_mitigation_acquisition_worklist.audited_axis_samples) !==
              Number(componentScalar.target_mitigation_evidence.audited_axis_samples) ||
            Number(componentScalar.target_mitigation_acquisition_worklist.strict_controlled_groups) !== 0 ||
            Number(componentScalar.target_mitigation_acquisition_worklist
              .target_status_relaxed_distinct_axis_pairs) !== 0 ||
            Number(componentScalar.target_mitigation_acquisition_worklist
              .pairs_with_effect_in_target_status_delta) !== 0 ||
            !Array.isArray(componentScalar.target_mitigation_acquisition_worklist
              .acquisition_contract?.required_controls) ||
            componentScalar.target_mitigation_acquisition_worklist.acquisition_contract
              .required_controls.length === 0 ||
            componentScalar.target_mitigation_acquisition_worklist
              .exact_target_mitigation_formula_proven !== false ||
            componentScalar.target_mitigation_acquisition_worklist
              .operation_order_and_integer_rounding_proven !== false ||
            componentScalar.target_mitigation_acquisition_worklist
              .packet_conservation_proven !== false ||
            componentScalar.target_mitigation_acquisition_worklist.formula_authority !== false ||
            componentScalar.target_mitigation_acquisition_worklist.runtime_authority !== false ||
            componentScalar.target_mitigation_acquisition_worklist
              .provider_rdps_credit_allowed !== false)) ||
        (componentScalar.target_mitigation_near_pair_candidate &&
          (componentScalar.target_mitigation_near_pair_candidate.status !==
            "exact-integer-candidate-compatible-status-confounded" ||
            componentScalar.target_mitigation_near_pair_candidate.model_id !==
              "target-physical-armor-counterfactual" ||
            Number(componentScalar.target_mitigation_near_pair_candidate
              .transformed_curve_constant) !== 22000 ||
            Number(componentScalar.target_mitigation_near_pair_candidate
              .runtime_simple_curve_constant) !== 6500 ||
            Number(componentScalar.target_mitigation_near_pair_candidate.packet_near_pair_rows) !== 3 ||
            Number(componentScalar.target_mitigation_near_pair_candidate
              .transformed_curve_compatible_rows) !== 3 ||
            JSON.stringify(componentScalar.target_mitigation_near_pair_candidate
              .transformed_curve_unique_shared_base_values) !== JSON.stringify(["107006"]) ||
            Number(componentScalar.target_mitigation_near_pair_candidate
              .runtime_simple_curve_compatible_rows) !== 0 ||
            componentScalar.target_mitigation_near_pair_candidate
              .selected_blade_sweep_effect_2110092_in_status_delta !== false ||
            componentScalar.target_mitigation_near_pair_candidate.exact_status_state_equal !== false ||
            componentScalar.target_mitigation_near_pair_candidate
              .effect_2201452_damage_stage_exclusivity_proven !== false ||
            componentScalar.target_mitigation_near_pair_candidate
              .exact_target_mitigation_formula_proven !== false ||
            componentScalar.target_mitigation_near_pair_candidate
              .operation_order_and_integer_rounding_proven !== false ||
            componentScalar.target_mitigation_near_pair_candidate.packet_conservation_proven !== false ||
            componentScalar.target_mitigation_near_pair_candidate.formula_authority !== false ||
            componentScalar.target_mitigation_near_pair_candidate.runtime_authority !== false ||
            componentScalar.target_mitigation_near_pair_candidate
              .provider_rdps_credit_allowed !== false ||
            (componentScalar.target_mitigation_near_pair_candidate
              .confounder_counterfactual_exhaustion !== undefined &&
              !isValidTargetMitigationStatusConfounderExhaustion(
                componentScalar.target_mitigation_near_pair_candidate
                  .confounder_counterfactual_exhaustion,
              )) ||
            (hasSameAxisStatusEvidence &&
              (componentScalar.target_mitigation_acquisition_worklist
                ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
                Number(componentScalar.target_mitigation_acquisition_worklist
                  ?.global_same_axis_target_status_pairs) !== 5 ||
                Number(componentScalar.target_mitigation_acquisition_worklist
                  ?.global_same_axis_equal_output_pairs) !== 4 ||
                Number(componentScalar.target_mitigation_acquisition_worklist
                  ?.global_same_axis_divergent_output_pairs) !== 1 ||
                Number(componentScalar.target_mitigation_near_pair_candidate
                  .same_axis_status_invariance?.physical_defense_same_axis_status_pairs) !== 5 ||
                Number(componentScalar.target_mitigation_near_pair_candidate
                  .same_axis_status_invariance?.physical_defense_same_axis_equal_output_pairs) !== 4 ||
                Number(componentScalar.target_mitigation_near_pair_candidate
                  .same_axis_status_invariance?.physical_defense_same_axis_divergent_output_pairs) !== 1 ||
                componentScalar.target_mitigation_near_pair_candidate.same_axis_status_invariance
                  ?.target_status_can_change_damage_outside_raw_defense !== true ||
                JSON.stringify(componentScalar.target_mitigation_near_pair_candidate
                  .same_axis_status_invariance?.candidate_status_effect_ids_without_same_axis_witness) !==
                JSON.stringify([55301, 2201452]))))) ||
        (hasCounterfactualDiscriminants &&
          (componentScalar.counterfactual_discriminants.status !==
            "exact-candidate-discriminants-awaiting-controlled-packet-proof" ||
            Number(componentScalar.counterfactual_discriminants
              .armor_penetration_basis_points) !== 650 ||
            Number(componentScalar.counterfactual_discriminants.defense_curve_constant) !== 22000 ||
            !Array.isArray(componentScalar.counterfactual_discriminants.exact_discriminant_rows) ||
            componentScalar.counterfactual_discriminants.exact_discriminant_rows.length !== 2 ||
            JSON.stringify(componentScalar.counterfactual_discriminants
              .distinct_predicted_damage_with_effect) !==
              JSON.stringify([85530, 85533, 87122, 87125]) ||
            componentScalar.counterfactual_discriminants.acquisition_contract
              ?.remote_player_packet_dependency !== false ||
            componentScalar.counterfactual_discriminants.exact_damage_projection_proven !== false ||
            componentScalar.counterfactual_discriminants.exact_operation_order_proven !== false ||
            componentScalar.counterfactual_discriminants.exact_integer_rounding_proven !== false ||
            componentScalar.counterfactual_discriminants.packet_conservation_proven !== false ||
            componentScalar.counterfactual_discriminants.formula_authority !== false ||
            componentScalar.counterfactual_discriminants.runtime_authority !== false ||
            componentScalar.counterfactual_discriminants.ui_display_authority !== false ||
            componentScalar.counterfactual_discriminants.provider_rdps_credit_allowed !== false)) ||
        (hasTargetStatusActionRouteAudit &&
          (componentScalar.target_status_action_route_audit.status !==
            "exact-produced-action-routes-audited-status-modifier-neutrality-unproven" ||
            Number(componentScalar.target_status_action_route_audit.audited_effects) !== 12 ||
            Number(componentScalar.target_status_action_route_audit
              .produced_damage_action_effects) !== 0 ||
            Number(componentScalar.target_status_action_route_audit
              .produced_action_healing_only_effects) !== 3 ||
            Number(componentScalar.target_status_action_route_audit
              .no_produced_action_observed_effects) !== 9 ||
            Number(componentScalar.target_status_action_route_audit
              .effects_eliminated_as_damage_neutral) !== 0 ||
            JSON.stringify(componentScalar.target_status_action_route_audit
              .candidate_near_pair_status_effects_without_same_axis_witness) !==
              JSON.stringify([55301, 2201452]) ||
            componentScalar.target_status_action_route_audit
              .status_modifier_damage_neutrality_proven !== false ||
            componentScalar.target_status_action_route_audit
              .target_status_confounders_eliminated !== false ||
            componentScalar.target_status_action_route_audit.formula_authority !== false ||
            componentScalar.target_status_action_route_audit.runtime_authority !== false ||
            componentScalar.target_status_action_route_audit.ui_display_authority !== false ||
            componentScalar.target_status_action_route_audit
              .provider_rdps_credit_allowed !== false)) ||
        (hasTargetDefensePercentLifecycleProof &&
          (componentScalar.target_defense_percent_lifecycle_proof.status !==
            "defense-stat-formula-proven-damage-counterfactual-unproven" ||
            Number(componentScalar.target_defense_percent_lifecycle_proof.effect_id) !== 2201452 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof.attribute_id) !== 11350 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof
              .percent_basis_points) !== 1000 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof
              .exact_wire_occurrences) !== 51 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof
              .application_occurrences) !== 30 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof
              .removal_occurrences) !== 21 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof
              .independent_sessions) !== 13 ||
            componentScalar.target_defense_percent_lifecycle_proof
              .effect_2201452_exact_defense_axis_mechanism_proven !== true ||
            componentScalar.target_defense_percent_lifecycle_proof
              .exact_target_defense_to_damage_formula_proven !== false ||
            componentScalar.target_defense_percent_lifecycle_proof
              .effect_2201452_damage_stage_exclusivity_proven !== false ||
            componentScalar.target_defense_percent_lifecycle_proof
              .hidden_additional_damage_stage_behavior_excluded !== false ||
            componentScalar.target_defense_percent_lifecycle_proof.formula_authority !== false ||
            componentScalar.target_defense_percent_lifecycle_proof.runtime_authority !== false ||
            componentScalar.target_defense_percent_lifecycle_proof.ui_display_authority !== false ||
            componentScalar.target_defense_percent_lifecycle_proof
              .provider_rdps_credit_allowed !== false)) ||
        (hasRawPercentLifecycleProof &&
          (Number(componentScalar.target_defense_percent_lifecycle_proof
            .packet_raw_percent_joined_occurrences) !== 47 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof
              .final_only_unresolved_occurrences) !== 4 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof
              .exact_family_input_transitions) !== 158 ||
            Number(componentScalar.target_defense_percent_lifecycle_proof
              .nearest_rounding_residual_mismatches) !== 86 ||
            componentScalar.target_defense_percent_lifecycle_proof
              .truncation_selected_over_round_to_nearest !== true ||
            componentScalar.target_defense_percent_lifecycle_proof
              .raw_percent_identity_for_all_lifecycle_occurrences_proven !== false)) ||
        (hasFightAttributeScopeProof &&
          (componentScalar.target_defense_fight_attribute_scope_proof?.status !==
            "complete-observed-fight-attribute-scope-hidden-damage-logic-unexcluded" ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.selected_fight_attribute_components) !== 906 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.components_with_exact_single_effect_same_wire_correlations) !== 26 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.proven_reversible_constant_components) !== 1 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.unresolved_fight_attribute_components) !== 25 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.only_proven_reversible_constant_attribute_id) !== 11354 ||
            JSON.stringify(componentScalar.target_defense_fight_attribute_scope_proof
              ?.unresolved_one_direction_attribute_ids) !== JSON.stringify([11710, 11711, 11712]) ||
            componentScalar.target_defense_fight_attribute_scope_proof
              ?.effect_is_defense_stat_only_across_observed_fight_attribute_components_proven !== false ||
            componentScalar.target_defense_fight_attribute_scope_proof
              ?.hidden_damage_stage_behavior_excluded !== false ||
            componentScalar.target_defense_fight_attribute_scope_proof?.formula_authority !== false ||
            componentScalar.target_defense_fight_attribute_scope_proof?.runtime_authority !== false ||
            componentScalar.target_defense_fight_attribute_scope_proof?.ui_display_authority !== false ||
            componentScalar.target_defense_fight_attribute_scope_proof
              ?.provider_rdps_credit_allowed !== false)) ||
        (hasCritCoTransitionProof &&
          (Number(componentScalar.target_defense_fight_attribute_scope_proof
            ?.raw_armor_presence_transitions) !== 47 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.raw_armor_applications) !== 26 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.raw_armor_removals) !== 21 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.raw_crit_add_application_co_updates) !== 0 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.raw_crit_add_removal_co_updates) !== 2 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.raw_armor_transitions_without_raw_crit_co_update) !== 45 ||
            Number(componentScalar.target_defense_fight_attribute_scope_proof
              ?.removal_only_raw_crit_add_delta) !== 50 ||
            componentScalar.target_defense_fight_attribute_scope_proof
              ?.unconditional_fixed_negative_50_raw_crit_add_component_supported !== false ||
            componentScalar.target_defense_fight_attribute_scope_proof
              ?.conditional_or_indirect_crit_behavior_excluded !== false)) ||
        (hasTargetDefenseStatusDiagnosticRollup &&
          (componentScalar.target_defense_status_diagnostic_rollup?.status !==
            "exhaustive-local-status-diagnostic-search-no-independent-control" ||
            Number(componentScalar.target_defense_status_diagnostic_rollup
              ?.matching_build_capture_diagnostics) !== 24 ||
            Number(componentScalar.target_defense_status_diagnostic_rollup?.damage_samples) !== 735016 ||
            Number(componentScalar.target_defense_status_diagnostic_rollup
              ?.physical_defense_unique_near_pairs) !== 3 ||
            Number(componentScalar.target_defense_status_diagnostic_rollup
              ?.physical_defense_pairs_with_selected_effect_in_status_delta) !== 3 ||
            Number(componentScalar.target_defense_status_diagnostic_rollup
              ?.physical_defense_same_axis_pairs_with_selected_effect_in_status_delta) !== 0 ||
            componentScalar.target_defense_status_diagnostic_rollup
              ?.no_new_independent_local_control_was_found !== true ||
            componentScalar.target_defense_status_diagnostic_rollup
              ?.remote_player_packet_acquisition_required !== false ||
            componentScalar.target_defense_status_diagnostic_rollup?.formula_authority !== false ||
            componentScalar.target_defense_status_diagnostic_rollup?.runtime_authority !== false ||
            componentScalar.target_defense_status_diagnostic_rollup?.ui_display_authority !== false ||
            componentScalar.target_defense_status_diagnostic_rollup
              ?.provider_rdps_credit_allowed !== false)) ||
        (hasTargetMitigationActorSceneExhaustion &&
          (componentScalar.target_mitigation_actor_scene_exhaustion?.status !==
            "exact-local-actor-scene-exhausted-no-cross-capture-control" ||
            Number(componentScalar.target_mitigation_actor_scene_exhaustion
              ?.selected_ability_samples) !== 185 ||
            Number(componentScalar.target_mitigation_actor_scene_exhaustion
              ?.physical_defense_samples) !== 23 ||
            Number(componentScalar.target_mitigation_actor_scene_exhaustion
              ?.physical_defense_samples_with_stable_target_actor_id) !== 0 ||
            Number(componentScalar.target_mitigation_actor_scene_exhaustion
              ?.cross_capture_actor_shape_pairs) !== 0 ||
            componentScalar.target_mitigation_actor_scene_exhaustion
              ?.structurally_unavailable_remote_player_packets_are_not_required !== true ||
            componentScalar.target_mitigation_actor_scene_exhaustion
              ?.missing_stable_remote_player_identity_is_preserved_not_synthesized !== true ||
            componentScalar.target_mitigation_actor_scene_exhaustion?.formula_authority !== false ||
            componentScalar.target_mitigation_actor_scene_exhaustion?.runtime_authority !== false ||
            componentScalar.target_mitigation_actor_scene_exhaustion?.ui_display_authority !== false ||
            componentScalar.target_mitigation_actor_scene_exhaustion
              ?.provider_rdps_credit_allowed !== false)) ||
        (hasRlogGapWindowAudit &&
          !isValidRlogGapWindowAudit(componentScalar.rlog_gap_window_audit)) ||
        (hasRlogTransitionCounterfactualAudit &&
          !isValidRlogTransitionCounterfactualAudit(
            componentScalar.rlog_transition_counterfactual_audit,
          )) ||
        (hasRlogTransitionMismatchFrontier &&
          !isValidRlogTransitionMismatchFrontier(
            componentScalar.rlog_transition_counterfactual_audit,
          )) ||
        (hasRlogTransitionStagedResidualFrontier &&
          !isValidRlogTransitionStagedResidualFrontier(
            componentScalar.rlog_transition_counterfactual_audit,
          )) ||
        (hasLuckyPacketComponentProof &&
          !isValidLuckyPacketComponentReceipt(
            componentScalar.lucky_packet_component_proof,
          )) ||
        (hasMAttackLuckyMitigationDiagnostic &&
          !isValidMAttackLuckyMitigationReceipt(
            componentScalar.mattack_lucky_mitigation_diagnostic,
          )) ||
        (hasAttackLuckyMitigationDiagnostic &&
          (!isValidAttackLuckyMitigationReceipt(
            componentScalar.attack_lucky_mitigation_diagnostic,
          ) ||
            !isValidAttackLuckyComponentRows(
              componentScalar.lucky_packet_component_proof?.attack_lucky_rows,
            ))) ||
        (hasLuckyParentMultiplierProof &&
          !(hasExhaustiveLocalSourceAttributeProof
            ? isValidExhaustiveLocalSourceAttributeLuckyReceipt(
              componentScalar.lucky_parent_multiplier_proof,
            )
            : hasGroupedLuckyRelationProof
              ? isValidGroupedLuckyParentMultiplierReceipt(
                componentScalar.lucky_parent_multiplier_proof,
              )
              : isValidLuckyParentMultiplierReceipt(
                componentScalar.lucky_parent_multiplier_proof,
              ))) ||
        (hasRlogOpaqueAttributeAudit &&
          !isValidRlogOpaqueAttributeAudit(componentScalar.rlog_opaque_attribute_audit)) ||
        (hasSourceStatusConfounderRouteAudit &&
          !isValidSourceStatusConfounderRouteAudit(
            componentScalar.source_status_confounder_route_audit,
          )) ||
        (hasSourceStatusLocalObservableAudit &&
          !isValidSourceStatusLocalObservableAudit(
            componentScalar.source_status_local_observable_audit,
          )) ||
        (hasTargetEffectFormulaProof &&
          !isValidTargetEffectFormulaProof(
            componentScalar.target_effect_formula_proof,
          )) ||
        (hasTargetDefenseTransformBoundary &&
          !isValidTargetDefenseTransformBoundary(
            componentScalar.target_defense_transform_boundary,
          )) ||
        !componentScalar.blockers?.every((blocker) => entry.blockers.includes(blocker)))) {
      throw new Error(`Unsafe component static scalar evidence for counterfactual frontier result ${key}`);
    }
  }
  if (counterfactualResults.length !== Number(report.summary?.counterfactual_frontier_effect_loci ?? 0) ||
    stableStringify(countBy(counterfactualResults, (entry) => entry.status)) !==
      stableStringify(report.summary?.counterfactual_frontier_status_counts ?? {})) {
    throw new Error("Counterfactual frontier summary mismatch");
  }
  const providerOwnedCounterfactualLoci = counterfactualResults.filter(
    (entry) => entry.provider_ownership_evidence?.run_scoped_player_ownership_proven === true,
  ).length;
  if (providerOwnedCounterfactualLoci !==
    Number(report.summary?.counterfactual_frontier_run_scoped_player_provider_owned_effect_loci ?? 0) ||
    providerOwnedCounterfactualLoci !== Number(
      report.summary?.attribution_progress
        ?.current_build_counterfactual_run_scoped_player_provider_owned_effect_loci ?? 0,
    )) {
    throw new Error("Counterfactual provider-ownership summary mismatch");
  }
  const integerTransformConstrainedLoci = counterfactualResults.filter(
    (entry) => entry.integer_transform_constraints?.exact_transform_proven === false,
  ).length;
  if (integerTransformConstrainedLoci !==
    Number(report.summary?.counterfactual_frontier_integer_transform_constrained_effect_loci ?? 0) ||
    integerTransformConstrainedLoci !== Number(
      report.summary?.attribution_progress
        ?.current_build_counterfactual_integer_transform_constrained_effect_loci ?? 0,
    )) {
    throw new Error("Counterfactual integer-transform summary mismatch");
  }
  if (counterfactualResults.length > 0 && report.summary?.strict_rdps_proof_complete === true) {
    throw new Error("Open counterfactual frontier evidence was excluded from strict proof completeness");
  }
  if (Number(report.summary?.packet_observed_runtime_attribution_promoted_exact_subset ?? 0) !==
    [...runtimeEffects.values()].filter((entry) => entry.status === "runtime-attribution-promoted-exact-subset").length) {
    throw new Error("Exact-subset runtime attribution promotion count mismatch");
  }
  const exactRuntimeSubsets = [...runtimeEffects.values()].filter((entry) =>
    entry.status === "runtime-attribution-promoted-exact-subset"
  );
  const expectedExactRuntimeEvents = exactRuntimeSubsets.reduce(
    (sum, entry) => sum + Number(entry.evidence.exact_replay_damage_events),
    0,
  );
  const expectedDeferredAmbiguousEvents = exactRuntimeSubsets.reduce(
    (sum, entry) => sum + Number(entry.evidence.ambiguous_provider_window_events),
    0,
  );
  const progress = report.summary?.attribution_progress;
  if (!progress ||
    Number(progress.current_build_exact_conserved_effect_subsets) !== exactRuntimeSubsets.length ||
    Number(progress.current_build_exact_conserved_damage_events) !== expectedExactRuntimeEvents ||
    Number(progress.current_build_full_effect_families_resolved) !==
      [...runtimeEffects.values()].filter((entry) => entry.full_effect_family_resolved === true).length ||
    Number(progress.current_build_fully_promotable_manifest_obligations) !==
      [...obligations.values()].filter((entry) => entry.status === "proven-promotable").length ||
    Number(progress.current_build_pending_manifest_obligations) !==
      [...obligations.values()].filter((entry) => !isClosedStatus(entry.status)).length ||
    Number(progress.historical_lead_only_manifest_obligations) !==
      [...obligations.values()].filter((entry) =>
        (entry.historical_proof_leads?.length ?? 0) > 0 && entry.status !== "proven-promotable"
      ).length ||
    Number(progress.deferred_ambiguous_provider_window_damage_events) !== expectedDeferredAmbiguousEvents ||
    (counterfactualResults.length > 0 && (
      Number(progress.current_build_counterfactual_frontier_open_effect_loci) !== counterfactualResults.length ||
      Number(progress.current_build_counterfactual_exact_controlled_effect_loci) !==
        counterfactualResults.filter((entry) => Number(entry.exact_recorded_inputs?.controlled_groups ?? 0) > 0).length ||
      Number(progress.current_build_counterfactual_exact_divergent_effect_loci) !==
        counterfactualResults.filter((entry) => Number(entry.exact_recorded_inputs?.divergent_output_groups ?? 0) > 0).length
    )) ||
    Number(progress.unresolved_evidence_hidden) !== 0) {
    throw new Error("Attribution progress summary mismatch");
  }
  const aggregateCorrelationIds = report.obligation_results.map((entry) => entry.correlation_match.aggregate_obligation_id);
  if (new Set(aggregateCorrelationIds).size !== aggregateCorrelationIds.length) {
    throw new Error("An aggregate obligation was correlated more than once");
  }
  const correlationCounts = countBy(report.obligation_results, (entry) => entry.correlation_match.kind);
  if (stableStringify(correlationCounts) !== stableStringify(report.summary?.correlation_match_counts ?? {})) {
    throw new Error("Aggregate correlation summary mismatch");
  }
  console.log(
    `rDPS proof closure verified for build ${report.game_build}: ${obligations.size} obligations, ` +
    `${models.size} shared models, ${report.summary.strictly_promotable_obligations} strictly promotable, zero hidden omissions.`,
  );
  return report;
}

function isValidSupportEffectFrontierResult(
  entry,
  closureSchemaVersion = PROOF_CLOSURE_SCHEMA_VERSION,
) {
  const common = Array.isArray(entry?.blockers) && entry.blockers.length > 0 &&
    entry?.formula_authority === false && entry?.runtime_authority === false &&
    entry?.ui_display_authority === false && entry?.provider_rdps_credit_allowed === false &&
    String(entry?.proof?.path ?? "") && Number(entry?.proof?.bytes) > 0 &&
    /^[0-9a-f]{64}$/.test(String(entry?.proof?.sha256 ?? ""));
  if (!common) return false;
  if (entry?.effect_id === "2202041") {
    const proofSchema = Number(entry.proof_schema_version);
    const lifecycleMetadataValid = proofSchema < 7 ||
      (Array.isArray(entry?.level_lifecycle_evidence) &&
        entry.level_lifecycle_evidence.length === entry.magnitude_by_level?.length &&
        entry.level_lifecycle_evidence.every((row) =>
          Number.isSafeInteger(Number(row?.level)) && Number(row.level) > 0 &&
          Number(row?.exact_instance_raw_delta) > 0 &&
          Array.isArray(row?.attributes) && row.attributes.length === 2 &&
          row.attributes.every((attribute) =>
            [11712, 11782].includes(Number(attribute?.attribute_id)) &&
            typeof attribute?.coefficient_consistent_with_instance_magnitude === "boolean" &&
            typeof attribute?.reversible_static_gate_passed === "boolean" &&
            typeof attribute?.matched_lifecycle_gate_passed === "boolean" &&
            (proofSchema < 8 || Array.isArray(attribute?.matched_examples))) &&
          typeof row?.reversible_static_transform_proven === "boolean" &&
          Array.isArray(row?.blockers)) &&
        Number(entry?.reversible_static_levels_proven) ===
          entry.level_lifecycle_evidence.filter(
            (row) => row.reversible_static_transform_proven === true,
          ).length &&
        entry?.all_observed_levels_reversible_static_transform_proven ===
          entry.level_lifecycle_evidence.every(
            (row) => row.reversible_static_transform_proven === true,
          ));
    const snapshotCoverage = entry?.formula_input_snapshot_coverage;
    const snapshotMetadataValid = proofSchema < 9 ||
      (snapshotCoverage?.event_time_snapshot_authority === false &&
        Number(snapshotCoverage?.exact_single_provider_candidate_events) ===
          Number(entry?.exact_single_provider_damage_events) &&
        Array.isArray(snapshotCoverage?.paths) && snapshotCoverage.paths.length === 3 &&
        Array.isArray(snapshotCoverage?.attributes) && snapshotCoverage.attributes.length ===
          (proofSchema >= 10 ? 4 : 3) &&
        Array.isArray(snapshotCoverage?.oldest_observed_examples) &&
        Array.isArray(snapshotCoverage?.missing_examples) &&
        Number(entry?.formula_input_events_with_complete_sets) ===
          snapshotCoverage.paths.reduce(
            (sum, row) => sum + Number(row.complete_input_sets), 0,
          ) &&
        Number(entry?.formula_input_events_all_wire_provenance) ===
          snapshotCoverage.paths.reduce(
            (sum, row) => sum + Number(row.all_inputs_wire_provenance), 0,
          ) &&
        Number(entry?.formula_input_events_observed_not_after_damage) ===
          snapshotCoverage.paths.reduce(
            (sum, row) => sum + Number(row.all_inputs_observed_not_after_damage), 0,
          ) &&
        entry?.event_time_formula_input_snapshot_proven === false);
    const integerStageCoverage = entry?.integer_stage_counterfactual_coverage;
    const interpretationMetadataValid = proofSchema < 17 || (() => {
      const rows = integerStageCoverage?.critical_factor_interpretation_breakdown;
      const validRelations = new Map([
        ["both", new Set(["same_exact", "divergent_exact", "within_interpretation_unresolved"])],
        ["additive_only", new Set(["single_interpretation_exact", "within_interpretation_unresolved"])],
        ["direct_only", new Set(["single_interpretation_exact", "within_interpretation_unresolved"])],
        ["neither", new Set(["no_compatible_interpretation"])],
      ]);
      if (!Array.isArray(rows) || rows.length === 0 ||
        integerStageCoverage?.critical_factor_interpretation_breakdown_authority !== false ||
        entry?.critical_factor_interpretation_breakdown_authority !== false ||
        entry?.critical_damage_raw_interpretation_authority !== false ||
        !Array.isArray(entry?.critical_factor_interpretation_breakdown) ||
        stableStringify(entry.critical_factor_interpretation_breakdown) !==
          stableStringify(rows)) {
        return false;
      }
      const expectedPathEvents = new Map(
        (integerStageCoverage?.paths ?? []).map((row) => [String(row?.path ?? ""), Number(row?.events)]),
      );
      if (expectedPathEvents.size !== 2 ||
        !expectedPathEvents.has("critical_proc_bonus") ||
        !expectedPathEvents.has("combined_lucky_occurrence_and_critical_bonus")) {
        return false;
      }
      const seen = new Set();
      const byPath = new Map();
      for (const row of rows) {
        const pathName = String(row?.path ?? "");
        const compatibility = String(row?.compatibility ?? "");
        const relation = String(row?.counterfactual_relation ?? "");
        const key = stableStringify([pathName, compatibility, relation]);
        const events = Number(row?.events);
        if (!expectedPathEvents.has(pathName) ||
          !validRelations.get(compatibility)?.has(relation) ||
          seen.has(key) || row?.formula_authority !== false ||
          !Number.isSafeInteger(events) || events <= 0) {
          return false;
        }
        seen.add(key);
        byPath.set(pathName, (byPath.get(pathName) ?? 0) + events);
      }
      const family = entry?.critical_damage_attribute_family ?? {};
      const interpretationReceiptValid = closureSchemaVersion < 24 ||
        (isValidFileDescriptor(entry?.critical_damage_factor_interpretation_proof) &&
        stableStringify([
          family.current_attribute_id, family.total_attribute_id, family.add_attribute_id,
          family.extra_add_attribute_id, family.percent_attribute_id,
          family.extra_percent_attribute_id,
        ]) === stableStringify([12510, 12511, 12512, 12513, 12514, 12515]) &&
        family.sync_to_local_player === true && family.sync_to_area_of_interest === false &&
        family.exact_static_sync_scope === "local-player-only-not-AOI" &&
        family.names_are_runtime_keys === false &&
        family.damage_consumer_semantics_proven === false &&
        entry?.critical_damage_exact_static_sync_scope === "local-player-only-not-AOI" &&
        entry?.remote_recipient_attribute_snapshots_required === false &&
        entry?.current_client_damage_factor_operator_present === false &&
        entry?.historical_critical_damage_formula_substitution_allowed === false);
      const runtimeGate = entry?.critical_damage_runtime_interpretation_gate ?? {};
      const runtimeGateValid = closureSchemaVersion < 25 ||
        (Number(runtimeGate.runtime_schema_version) ===
          (closureSchemaVersion >= 31 ? 5 : closureSchemaVersion >= 27 ? 4 : 3) &&
          (closureSchemaVersion < 27 ||
            stableStringify(runtimeGate.promotion_blockers) === stableStringify([
              "protocol-pack-identity",
              "canonical-replay-conservation",
              "protocol-event-coverage",
              "critical-damage-factor-interpretation-authority",
              ...(closureSchemaVersion >= 31 ? ["party-support-formula-frontier"] : []),
            ])) &&
          runtimeGate.configured_interpretation === "unresolved" &&
          runtimeGate.configured_interpretation_authority === false &&
          runtimeGate.candidate_rules_enabled === false &&
          runtimeGate.runtime_promotion_allowed === false &&
          runtimeGate.inspiration_runtime_transfer_enabled === false &&
          runtimeGate.interpretation_and_authority_must_advance_together === true &&
          runtimeGate.unresolved_interpretation_blocks_critical_dependent_projection === true &&
          stableStringify(runtimeGate.retained_candidate_arithmetic_implemented) ===
            stableStringify(["additive_bonus", "direct_total"]) &&
          runtimeGate.candidate_arithmetic_formula_authority === false);
      const remoteEvidenceBoundaryValid = closureSchemaVersion < 26 ||
        (entry?.remote_player_cast_packets_required === false &&
          entry?.remote_player_cast_packets_treated_as_zero === false &&
          entry?.remote_player_cast_packets_synthesized === false &&
          entry?.remote_recipient_attribute_snapshots_required === false &&
          entry?.current_player_attribute_snapshots_substituted_for_remote_players === false);
      const controlledPairReceiptValid = closureSchemaVersion < 28 ||
        (isValidFileDescriptor(entry?.critical_factor_controlled_pair_discriminant) &&
          isValidControlledPairDiscriminantAudit(
            entry?.critical_factor_controlled_pair_audit,
            closureSchemaVersion >= 29 ? 2 : 1,
          ));
      return interpretationReceiptValid && runtimeGateValid && remoteEvidenceBoundaryValid &&
        controlledPairReceiptValid &&
        [...expectedPathEvents.entries()].every(
        ([pathName, events]) => byPath.get(pathName) === events,
      ) && rows.reduce((sum, row) => sum + Number(row.events), 0) ===
        Number(entry?.integer_stage_critical_events);
    })();
    const integerStageMetadataValid = proofSchema < 11 ||
      (integerStageCoverage?.candidate_family_authority === false &&
        integerStageCoverage?.counterfactual_authority === false &&
        Array.isArray(integerStageCoverage?.candidate_family) &&
        integerStageCoverage.candidate_family.length === (proofSchema >= 17 ? 6 : 3) &&
        Array.isArray(integerStageCoverage?.paths) &&
        integerStageCoverage.paths.length === 2 &&
        Array.isArray(integerStageCoverage?.exact_examples) &&
        Array.isArray(integerStageCoverage?.unresolved_examples) &&
        Number(integerStageCoverage?.exact_single_provider_candidate_events) ===
          Number(entry?.exact_single_provider_damage_events) &&
        Number(integerStageCoverage?.critical_stage_events) ===
          Number(entry?.integer_stage_critical_events) &&
        Number(integerStageCoverage?.events_with_at_least_one_compatible_candidate) ===
          Number(entry?.integer_stage_events_with_compatible_candidates) &&
        Number(integerStageCoverage?.exact_stage_independent_events) ===
          Number(entry?.integer_stage_exact_counterfactual_events) &&
        Number(integerStageCoverage?.unresolved_stage_or_rounding_events) ===
          Number(entry?.integer_stage_unresolved_order_or_rounding_events) &&
        Number(integerStageCoverage?.events_without_compatible_candidates) ===
          Number(entry?.integer_stage_events_without_compatible_candidates) &&
        (proofSchema < 18 ||
          (Number(integerStageCoverage?.critical_factor_event_records_retained_in_bound_proof) ===
              Number(entry?.integer_stage_critical_events) &&
            integerStageCoverage?.critical_factor_event_records_inlined_in_closure === false &&
            integerStageCoverage?.critical_factor_event_records === undefined)) &&
        Number(entry?.integer_stage_exact_counterfactual_events) +
          Number(entry?.integer_stage_unresolved_order_or_rounding_events) ===
          Number(entry?.integer_stage_events_with_compatible_candidates) &&
        entry?.integer_stage_candidate_family_proven === false &&
        entry?.integer_stage_counterfactual_proven === false &&
        interpretationMetadataValid);
    const damageSurfaceMetadataValid = proofSchema < 12 || (() => {
      if (!String(entry?.damage_formula_surface?.path ?? "") ||
        !Number.isSafeInteger(Number(entry?.damage_formula_surface?.bytes)) ||
        Number(entry.damage_formula_surface.bytes) <= 0 ||
        !/^[0-9a-f]{64}$/.test(String(entry?.damage_formula_surface?.sha256 ?? "")) ||
        ![1, 2].includes(Number(entry?.damage_formula_surface_schema_version)) ||
        entry?.damage_formula_surface_runtime_authority !== false ||
        entry?.damage_script_identity_proves_operator !== false ||
        entry?.owner_stage_array_selection_proves_formula !== false) {
        return false;
      }
      const audited = validateInspirationDamageSurfaceJoin(entry?.damage_surface_join, {
        criticalEvents: Number(entry?.integer_stage_critical_events),
        compatibleEvents: Number(entry?.integer_stage_events_with_compatible_candidates),
        exactEvents: Number(entry?.integer_stage_exact_counterfactual_events),
        unresolvedEvents: Number(entry?.integer_stage_unresolved_order_or_rounding_events),
        noCompatibleEvents: Number(entry?.integer_stage_events_without_compatible_candidates),
        exactStageInputIdentity: proofSchema >= 15,
        exactStageInputFreshness: proofSchema >= 16,
      });
      return Number(entry?.damage_surface_identity_groups) === audited.identityGroups &&
        Number(entry?.damage_surface_events_with_unique_row) === audited.uniqueRowEvents &&
        Number(entry?.damage_surface_events_with_ambiguous_rows) === audited.ambiguousRowEvents &&
        Number(entry?.damage_surface_events_without_row) === audited.missingRowEvents &&
        Number(entry?.damage_surface_events_with_resolved_script) ===
          audited.resolvedScriptEvents &&
        Number(entry?.damage_surface_events_without_resolved_script) ===
          audited.unresolvedScriptEvents &&
        (proofSchema < 13 ||
          (Number(entry?.damage_surface_events_with_unique_ability_candidate_when_hit_event_absent) ===
              audited.uniqueAbilityDiagnosticEvents &&
            Number(entry?.damage_surface_events_with_unique_ability_candidate_and_resolved_script_when_hit_event_absent) ===
              audited.uniqueAbilityResolvedScriptDiagnosticEvents &&
            Number(entry?.damage_surface_events_without_exact_or_unique_ability_candidate) ===
              audited.remainingWithoutExactOrUniqueEvents &&
            entry?.unique_ability_damage_surface_candidate_authority === false &&
            entry?.missing_hit_event_id_synthesized === false)) &&
        (proofSchema < 14 ||
          (Array.isArray(entry?.damage_script_preimage_breakdown) &&
            entry.damage_script_preimage_breakdown.length ===
              audited.damageScriptPreimageBreakdownRows &&
            stableStringify(entry.damage_script_preimage_breakdown) ===
              stableStringify(entry.damage_surface_join.damage_script_preimage_breakdown) &&
            entry?.damage_script_preimage_breakdown_authority === false)) &&
        (proofSchema < 15 ||
          entry?.damage_surface_identity_groups_include_exact_stage_inputs === true) &&
        (proofSchema < 16 ||
          (Array.isArray(entry?.stage_input_freshness_breakdown) &&
            entry.stage_input_freshness_breakdown.length ===
              audited.stageInputFreshnessBreakdownRows &&
            stableStringify(entry.stage_input_freshness_breakdown) ===
              stableStringify(entry.damage_surface_join.stage_input_freshness_breakdown) &&
            entry?.stage_input_freshness_breakdown_authority === false &&
            entry?.damage_surface_identity_groups_include_exact_stage_input_freshness === true));
    })();
    const cohortMetadataValid = entry?.proof_schema_version === undefined ||
      ([3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18].includes(proofSchema) &&
        Number(entry?.exact_rlogs) > 0 &&
        Number(entry?.independent_sessions_with_magnitudes) > 0 &&
        Array.isArray(entry?.magnitude_raw_deltas) && entry.magnitude_raw_deltas.length > 0 &&
        entry.magnitude_raw_deltas.every((value) => Number(value) > 0) &&
        (Number(entry.proof_schema_version) < 5 ||
          (Array.isArray(entry?.magnitude_by_level) && entry.magnitude_by_level.length > 0 &&
            entry.magnitude_by_level.every((row) =>
              Number.isSafeInteger(Number(row?.level)) && Number(row.level) > 0 &&
              Number(row?.raw_delta) > 0 && Number(row?.exact_instances) > 0 &&
              Number(row?.independent_sessions) > 0)) &&
        (Number(entry.proof_schema_version) < 6 ||
          (entry?.parent_effect_id === "2202040" &&
            Number(entry?.exact_origin_source_type_id) === 1 &&
            entry?.exact_origin_source_config_id === "2202040")) &&
        lifecycleMetadataValid && snapshotMetadataValid && integerStageMetadataValid &&
          damageSurfaceMetadataValid));
    return cohortMetadataValid && entry?.mechanic === "critical-and-lucky-proc-chance" &&
      entry?.exact_stat_transform_proven === false &&
      entry?.exact_instance_magnitudes_proven === true &&
      Number(entry?.proven_instance_windows) > 0 &&
      Number(entry?.exact_packet_duration_rows) > 0 &&
      Number(entry?.exact_single_provider_damage_events) > 0 &&
      Number(entry?.critical_only_diagnostic_events) +
        Number(entry?.lucky_only_diagnostic_events) +
        Number(entry?.combined_diagnostic_events) ===
        (proofSchema >= 18
          ? Number(entry?.exact_rational_contribution_events)
          : Number(entry?.exact_single_provider_damage_events)) &&
      (proofSchema < 18 ||
        (Number(entry?.exact_rational_contribution_events) > 0 &&
          Number(entry?.critical_interpretation_blocked_candidate_events) > 0 &&
          Number(entry.exact_rational_contribution_events) +
            Number(entry.critical_interpretation_blocked_candidate_events) ===
              Number(entry.exact_single_provider_damage_events))) &&
      entry?.exact_rational_bucket_arithmetic_conserved === true &&
      entry?.opportunity_counterfactual_proven === false &&
      Number(entry?.observed_damage_reassigned_to_provider) === 0;
  }
  if (entry?.effect_id === "31602") {
    return entry?.mechanic === "party-haste-percent-status-coefficient" &&
      entry?.proof_state ===
        "exact-current-build-effect-to-raw-haste-percent-family-coefficient-proven-downstream-semantics-open" &&
      [28, 29].includes(Number(entry?.proof_schema_version)) &&
      entry?.exact_stat_transform_proven === true &&
      stableStringify(entry?.changed_attribute_ids) ===
        stableStringify(["11930", "11931", "11932"]) &&
      Number(entry?.exact_raw_additive_coefficient_units) === 1000 &&
      Number(entry?.apply_occurrences) === 3 && Number(entry?.remove_occurrences) === 2 &&
      Number(entry?.independent_run_contexts) === 2 &&
      Number(entry?.cross_actor_occurrences) === 5 &&
      Number(entry?.missing_source_occurrences) === 0 &&
      Number(entry?.exact_origin?.source_type_id) === 1 &&
      Number(entry?.exact_origin?.source_config_id) === 31601 &&
      entry?.raw_unit_interpretation_authority === false &&
      entry?.stacking_arbitration_proven === false &&
      entry?.opportunity_counterfactual_proven === false &&
      Number(entry?.observed_damage_reassigned_to_provider) === 0;
  }
  return entry?.effect_id === "2207252" &&
    entry?.mechanic === "haste-action-opportunity" &&
    entry?.exact_stat_transform_proven === true &&
    entry?.raw_attribute_id === "11120" &&
    entry?.transformed_attribute_id === "11930" &&
    Number(entry?.packet_fixed_point_scale) === 10000 &&
    entry?.row_3_delta_formula ===
      "trunc_or_floor(10000 * raw_haste / (raw_haste + 50000))" &&
    Number(entry?.exact_delta_batches) > 0 &&
    stableStringify(entry?.absolute_additive_residuals) === stableStringify([0, 2250]) &&
    Number(entry?.gap_bounded_lifecycles) > 0 &&
    Number(entry?.gap_bounded_windows_with_damage) > 0 &&
    Number(entry?.gap_bounded_windows_with_damage) <= Number(entry?.gap_bounded_lifecycles) &&
    Number(entry?.observed_damage_events_while_active) > 0 &&
    Number(entry?.observed_action_start_events) === 0 &&
    entry?.action_start_coverage_observed === false &&
    entry?.opportunity_counterfactual_proven === false &&
    Number(entry?.observed_damage_reassigned_to_provider) === 0;
}

function verifyProductionReadiness(report) {
  if (report.policy?.exact_subset_replay_proof_is_not_production_runtime_promotion !== true ||
    report.policy?.production_runtime_credit_requires_promoted_matching_build_protocol_pack !== true ||
    report.policy?.protocol_event_coverage_requires_only_locally_observable_exact_routes !== true ||
    report.policy
      ?.structural_remote_packet_non_obligations_never_synthesize_or_zero_fill_events !== true) {
    throw new Error("rDPS production-readiness policy is unsafe");
  }
  const readiness = report.production_readiness;
  if (!readiness || !Array.isArray(readiness.blockers) ||
    stableStringify(readiness.required_proof_suites ?? []) !==
      stableStringify(["canonical-replay-conservation", "protocol-event-coverage"])) {
    throw new Error("rDPS production-readiness gate is incomplete");
  }
  const structuralNonObligations = readiness.structural_remote_packet_non_obligations;
  if (readiness.protocol_event_coverage_scope !== "locally-observable-exact-routes" ||
    !Array.isArray(structuralNonObligations) ||
    structuralNonObligations.length !==
      Number(readiness.structural_remote_packet_non_obligation_count ?? -1) ||
    structuralNonObligations.some((route) =>
      !Number.isSafeInteger(Number(route.service_id)) || Number(route.service_id) <= 0 ||
      !Number.isSafeInteger(Number(route.method_id)) || Number(route.method_id) <= 0 ||
      Number(route.packet_count) !== 0 || Number(route.decoded_records) !== 0 ||
      route.promotion_requirement_satisfied !== true || !String(route.reason ?? "")
    )) {
    throw new Error("rDPS production-readiness structural non-obligation accounting is unsafe");
  }
  const protocolPromoted = readiness.protocol_pack_status === "promoted" &&
    readiness.protocol_pack_identity_present === true &&
    readiness.protocol_pack_identity_build_matches === true &&
    readiness.protocol_pack_byte_identical_to_audited_candidate === true;
  if (readiness.runtime_promotion_allowed !== protocolPromoted) {
    throw new Error("Protocol-pack runtime-promotion gate is inconsistent");
  }
  const strictComplete = report.summary?.strict_rdps_proof_complete === true;
  const supportEffectFrontierComplete =
    report.summary?.support_effect_frontier_complete === true;
  const partySkillStaticFrontierComplete = Number(report.schema_version) >= 30
    ? report.summary?.party_skill_static_frontier_complete === true
    : true;
  const partyEffectWindowFrontierComplete = Number(report.schema_version) >= 31
    ? report.summary?.party_effect_window_frontier_complete === true
    : true;
  const lifeWaveTriggerFrontierComplete = Number(report.schema_version) >= 77
    ? report.summary?.life_wave_trigger_frontier_complete === true
    : true;
  const lifeWaveRemoteInferenceFrontierComplete = Number(report.schema_version) >= 78
    ? report.summary?.life_wave_remote_inference_frontier_complete === true
    : true;
  const productionReady = protocolPromoted && strictComplete && supportEffectFrontierComplete &&
    partySkillStaticFrontierComplete && partyEffectWindowFrontierComplete &&
    lifeWaveTriggerFrontierComplete && lifeWaveRemoteInferenceFrontierComplete;
  if (readiness.strict_rdps_proof_complete !== strictComplete ||
    (Number(report.schema_version) >= 30 &&
      (readiness.support_effect_frontier_complete !== supportEffectFrontierComplete ||
        readiness.party_skill_static_frontier_complete !== partySkillStaticFrontierComplete)) ||
    (Number(report.schema_version) >= 31 &&
      readiness.party_effect_window_frontier_complete !== partyEffectWindowFrontierComplete) ||
    (Number(report.schema_version) >= 77 &&
      readiness.life_wave_trigger_frontier_complete !== lifeWaveTriggerFrontierComplete) ||
    (Number(report.schema_version) >= 78 &&
      readiness.life_wave_remote_inference_frontier_complete !==
        lifeWaveRemoteInferenceFrontierComplete) ||
    readiness.production_runtime_ready !== productionReady ||
    report.summary?.production_runtime_ready !== productionReady) {
    throw new Error("Production runtime readiness summary is inconsistent");
  }
  const runtimePromotable = protocolPromoted
    ? Number(report.summary?.strictly_promotable_obligations ?? 0)
    : 0;
  if (Number(report.summary?.runtime_promotable_obligations ?? -1) !== runtimePromotable) {
    throw new Error("Runtime-promotable obligation summary bypasses the protocol-pack gate");
  }
  const productionCreditEffects = (report.packet_observed_runtime_effect_results ?? []).filter(
    (effect) => effect.production_runtime_credit_allowed === true,
  );
  if (productionCreditEffects.some((effect) =>
    !protocolPromoted || effect.status !== "runtime-attribution-promoted-exact-subset"
  )) {
    throw new Error("Production runtime credit bypasses the protocol-pack gate");
  }
  if (productionCreditEffects.length !==
    Number(report.summary?.packet_observed_runtime_production_credit_allowed_effects ?? -1)) {
    throw new Error("Production runtime credit summary mismatch");
  }
  if (!protocolPromoted && (report.packet_observed_runtime_effect_results ?? []).some(
    (effect) => effect.production_runtime_credit_allowed !== false,
  )) {
    throw new Error("Blocked protocol-pack status did not fail closed for runtime credit");
  }
}

function inspect(input, parsed) {
  const report = verify(input);
  const limit = parsed.limit === undefined ? 20 : positiveInteger(parsed.limit, "limit");
  if (parsed.model) {
    const models = report.shared_model_results.filter((entry) => entry.model_key === parsed.model);
    if (!models.length) throw new Error(`Unknown model ${parsed.model}`);
    console.log(JSON.stringify(models[0], null, 2));
    return;
  }
  let entries = report.obligation_results;
  if (parsed.status) entries = entries.filter((entry) => entry.status === parsed.status);
  console.log(`\nrDPS closure build ${report.game_build}: showing ${Math.min(limit, entries.length)} of ${entries.length} obligations`);
  console.log(JSON.stringify(report.summary, null, 2));
  for (const entry of entries.slice(0, limit)) {
    console.log(`\n${entry.obligation_id} | ${entry.status}`);
    console.log(`  ${entry.subject_name}`);
    console.log(`  Blockers: ${entry.blockers.length ? entry.blockers.join(", ") : "none"}`);
    if (entry.shared_model_keys.length) console.log(`  Shared models: ${entry.shared_model_keys.join(", ")}`);
  }
}

function selfTest() {
  const targetRole = resolvePartyEffectAffectedEntityRole({
    statusEvents: 2,
    supportCategories: ["external-target-vulnerability"],
    identityEvidence: {
      affected_entity_status_events: 2,
      affected_entity_identity_unresolved_events: 0,
    },
    externalAffectedEntityPartyMembershipProvenForEveryStatusEvent: false,
    actorRelation: { event_count: 0 },
    targetRelation: { event_count: 3 },
    damageActionEdgeSummary: {
      effect_target_as_damage_actor_edges: 0,
      effect_target_as_damage_actor_event_references: 0,
      effect_target_as_damage_target_edges: 1,
      effect_target_as_damage_target_event_references: 3,
    },
  });
  const recipientRole = resolvePartyEffectAffectedEntityRole({
    statusEvents: 2,
    supportCategories: ["party-offensive-stat"],
    identityEvidence: {
      affected_entity_status_events: 2,
      affected_entity_identity_unresolved_events: 0,
    },
    externalAffectedEntityPartyMembershipProvenForEveryStatusEvent: true,
    actorRelation: { event_count: 4 },
    targetRelation: { event_count: 0 },
    damageActionEdgeSummary: {
      effect_target_as_damage_actor_edges: 1,
      effect_target_as_damage_actor_event_references: 4,
      effect_target_as_damage_target_edges: 0,
      effect_target_as_damage_target_event_references: 0,
    },
  });
  if (targetRole.proven !== true || targetRole.resolution !== "damage-target-allegiance-neutral" ||
    targetRole.requires_party_membership !== false || recipientRole.proven !== true ||
    recipientRole.resolution !== "damage-actor-allegiance-neutral" ||
    recipientRole.requires_party_membership !== false) {
    throw new Error("Self-test did not preserve allegiance-neutral effect endpoint roles");
  }
  const repeatedRollups = parseArgs([
    "--counterfactual-rollup", "first.json",
    "--counterfactual-rollup", "second.json",
  ])["counterfactual-rollup"];
  if (repeatedRollups?.join(",") !== "first.json,second.json") {
    throw new Error("Self-test did not preserve repeated counterfactual rollup inputs");
  }
  const repeatedOwnershipProofs = parseArgs([
    "--provider-ownership-proof", "first.json",
    "--provider-ownership-proof", "second.json",
  ])["provider-ownership-proof"];
  if (repeatedOwnershipProofs?.join(",") !== "first.json,second.json") {
    throw new Error("Self-test did not preserve repeated provider ownership proof inputs");
  }
  const repeatedTransformConstraints = parseArgs([
    "--integer-transform-constraints", "first.json",
    "--integer-transform-constraints", "second.json",
  ])["integer-transform-constraints"];
  if (repeatedTransformConstraints?.join(",") !== "first.json,second.json") {
    throw new Error("Self-test did not preserve repeated integer transform constraint inputs");
  }
  const repeatedComponentScalarProofs = parseArgs([
    "--component-scalar-proof", "first.json",
    "--component-scalar-proof", "second.json",
  ])["component-scalar-proof"];
  if (repeatedComponentScalarProofs?.join(",") !== "first.json,second.json") {
    throw new Error("Self-test did not preserve repeated component scalar proof inputs");
  }
  const repeatedSupportEffectProofs = parseArgs([
    "--support-effect-proof", "first.json",
    "--support-effect-proof", "second.json",
  ])["support-effect-proof"];
  if (repeatedSupportEffectProofs?.join(",") !== "first.json,second.json") {
    throw new Error("Self-test did not preserve repeated support-effect proof inputs");
  }
  const supportEffectFixture = {
    effect_id: "2207252",
    mechanic: "haste-action-opportunity",
    exact_stat_transform_proven: true,
    raw_attribute_id: "11120",
    transformed_attribute_id: "11930",
    packet_fixed_point_scale: 10000,
    row_3_delta_formula: "trunc_or_floor(10000 * raw_haste / (raw_haste + 50000))",
    exact_delta_batches: 5,
    absolute_additive_residuals: [0, 2250],
    gap_bounded_lifecycles: 9,
    gap_bounded_windows_with_damage: 2,
    observed_damage_events_while_active: 4,
    observed_action_start_events: 0,
    action_start_coverage_observed: false,
    opportunity_counterfactual_proven: false,
    observed_damage_reassigned_to_provider: 0,
    blockers: ["action-start coverage unavailable"],
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
    proof: { path: "fixture.json", bytes: 1, sha256: "a".repeat(64) },
  };
  if (!isValidSupportEffectFrontierResult(supportEffectFixture)) {
    throw new Error("Self-test rejected a fail-closed support-effect frontier result");
  }
  const unsafeSupportEffectFixture = {
    ...supportEffectFixture,
    observed_damage_reassigned_to_provider: 1,
    provider_rdps_credit_allowed: true,
  };
  if (isValidSupportEffectFrontierResult(unsafeSupportEffectFixture)) {
    throw new Error("Self-test accepted unsupported provider credit for an unobservable opportunity");
  }
  if (!isCompleteFormulaSnapshot({ state: "complete" })) {
    throw new Error("Self-test did not recognize the Rust formula snapshot state field");
  }
  const projectedFormulaCounts = formulaSnapshotCounts({
    formula_input_snapshot_count: 5,
    complete_formula_input_snapshot_count: 4,
    formula_input_snapshots: [{ state: "incomplete" }],
  });
  if (projectedFormulaCounts.total !== 5 || projectedFormulaCounts.complete !== 4) {
    throw new Error("Self-test did not preserve closure-projected formula snapshot counts");
  }
  if (projectedCount(7, 0) !== 7 || projectedCount(undefined, 3) !== 3) {
    throw new Error("Self-test did not preserve closure-projected evidence counts");
  }
  const scalarOwnershipHold = evaluateObligation({
    build: "1",
    manifestObligation: {
      obligation_id: "component-scalar-ownership-hold",
      domain: "runtime",
      subject_kind: "effect",
      subject_id: "2110092",
      subject_name: "exact numeric effect 2110092",
      required_event_kinds: ["actor", "damage", "status"],
      selectors: { effect_ids: [2110092] },
      evidence: { component_kind: "external-target-state-counterfactual" },
    },
    observed: {
      coverage_state: "candidate-event-coverage-complete",
      observed_event_kinds: ["actor", "damage", "status"],
      missing_event_kinds: [],
      direct_matches: 1,
      contextual_matches: 0,
      provider_recipient_observations: [{ provider_actor_id: "1", recipient_actor_id: "2" }],
      status_states: { applied: 1, removed: 1 },
      ambiguous_status_removals: 0,
      ambiguous_provider_window_damage_events: 0,
    },
    aggregate: {
      manifest_game_build: "1",
      observed_game_builds: ["1"],
      provisional_build_mismatch: false,
    },
    sourceRuleIds: [],
    staticBySource: new Map(),
    workbenchModelsBySource: new Map(),
    historicalProofsByEffect: new Map(),
    componentScalarProofIndex: new Map([[2110092, {
      proof: { path: "component-scalar.json", bytes: 1, sha256: "a".repeat(64) },
      exact_provider_ownership_proven: false,
      unresolved_provider_status_events: 53,
    }]]),
  });
  if (scalarOwnershipHold.status !== "observed-external-scope-awaiting-provider-ownership" ||
    scalarOwnershipHold.gates.exact_component_scalar_provider_ownership !== false ||
    scalarOwnershipHold.evidence.component_static_scalar_provider_ownership
      ?.unresolved_provider_status_events !== 53 ||
    !scalarOwnershipHold.blockers.includes("component-static-scalar-provider-ownership-unproven")) {
    throw new Error("Self-test did not fail closed on component scalar provider ownership");
  }

  const root = mkdtempSync(path.join(tmpdir(), "rlogs-rdps-proof-closure-"));
  try {
    const files = {
      manifest: path.join(root, "manifest.json"),
      aggregate: path.join(root, "aggregate.json"),
      staticFormulaEvidence: path.join(root, "static.json"),
      workbench: path.join(root, "workbench.json"),
      carryForward: path.join(root, "carry.json"),
      runtimeAttributionEvidence: path.join(root, "runtime-attribution.json"),
      counterfactualFrontier: path.join(root, "counterfactual-frontier.json"),
      counterfactualRollup: path.join(root, "counterfactual-rollup.json"),
      providerOwnershipProof: path.join(root, "provider-ownership-proof.json"),
      runtimeEffectComponentRoutingProof: path.join(root, "runtime-effect-routing.json"),
      partySkillStaticClosure: path.join(root, "party-skill-static-closure.json"),
      partyEffectWindowAudit: path.join(root, "party-effect-window-audit.json"),
      protocolStatus: path.join(root, "protocol-status.json"),
      output: path.join(root, "closure.json"),
    };
    writeJson(files.manifest, {
      game_build: "1", summary: { frontier_work_items: 1, indexed_obligations: 2, explicitly_unindexable_obligations: 0 }, obligations: [
        { obligation_id: "a", domain: "runtime", subject_kind: "x", subject_id: "1", subject_name: "A", required_event_kinds: ["actor", "damage", "status"], selectors: { source_rule_ids: ["mrs:a"], effect_ids: [3003012] }, evidence: { component_kind: "external-recipient-counterfactual" } },
        { obligation_id: "b", domain: "runtime", subject_kind: "x", subject_id: "2", subject_name: "B", required_event_kinds: ["actor", "damage", "status"], selectors: { source_rule_ids: ["mrs:a"], effect_ids: [2] }, evidence: { component_kind: "source-owned-output-nontransfer" } },
      ],
    });
    const observedBase = {
      observed_event_kinds: ["actor", "damage", "status"], missing_event_kinds: [], direct_matches: 1, contextual_matches: 0,
      status_states: { applied: 1, removed: 1 }, ambiguous_status_removals: 0, ambiguous_provider_window_damage_events: 0,
      recipient_window_damage_events: 1, target_window_damage_events: 1, single_provider_window_damage_events: 1,
      formula_input_snapshots: [], packet_damage_rows: [], projection_statuses: [], projected_provider_recipient_observations: [],
      projected_integer_events: 0, projected_rational_events: 0, projected_invalid_events: 0, projected_excluded_events: 0,
    };
    writeJson(files.aggregate, { aggregate: {
      manifest_game_build: "1", observed_game_builds: ["1"], provisional_build_mismatch: false, total_events: 4,
      summary: { candidate_event_coverage_complete: 1 }, obligations: [
        {
          ...observedBase,
          obligation_id: "old-a",
          domain: "runtime",
          subject_kind: "x",
          subject_id: "1",
          required_event_kinds: ["actor", "damage", "status"],
          selector_contract: JSON.stringify({ source_rule_ids: ["mrs:old-a"], effect_ids: [3003012] }),
          coverage_state: "partial-candidate-event-coverage",
          observed_event_kinds: ["actor"],
          missing_event_kinds: ["damage", "status"],
          status_states: {},
          recipient_window_damage_events: 0,
          target_window_damage_events: 0,
          single_provider_window_damage_events: 0,
          provider_recipient_observations: [],
        },
        { ...observedBase, obligation_id: "b", coverage_state: "no-candidate-evidence", direct_matches: 0, provider_recipient_observations: [] },
      ],
      dreamscope_terminal_effects: [
        {
          effect_id: 3003012,
          source_match: { resolution: "exact" },
          status_states: { applied: 1, removed: 1 },
          provider_recipient_observations: [{ provider_actor_id: "1", recipient_actor_id: "2", observation_count: 2 }],
          status_instance_ids: ["status-1"],
          source_observations: [{ route_resolution: "exact" }],
          ambiguous_status_removals: 0,
          ambiguous_provider_window_damage_events: 0,
          recipient_window_damage_events: 1,
          target_window_damage_events: 1,
          single_provider_window_damage_events: 1,
          external_provider_window_damage_events: 1,
          external_provider_window_damage: "10",
        },
        {
          effect_id: 3003014,
          source_match: { resolution: "exact" },
          status_states: { applied: 1, removed: 1 },
          provider_recipient_observations: [{ provider_actor_id: "1", recipient_actor_id: "2" }],
          source_observations: [{ route_resolution: "exact" }],
          ambiguous_status_removals: 0,
          ambiguous_provider_window_damage_events: 0,
          external_provider_window_damage_events: 1,
          external_provider_window_damage: "10",
        },
      ],
      remote_rdps_readiness: { effects: [
        {
          effect_id: 3003012,
          external_attribution_candidate: true,
          scalar_resolution: "unresolved",
          retained_external_provider_window_damage_events: 1,
          retained_external_provider_window_damage: "10",
          calculation_ready: false,
        },
        {
          effect_id: 3003014,
          external_attribution_candidate: true,
          scalar_resolution: "static",
          retained_external_provider_window_damage_events: 1,
          retained_external_provider_window_damage: "10",
          calculation_ready: false,
        },
      ] },
    } });
    writeJson(files.staticFormulaEvidence, { game_build: "1", summary: { sources: 1, static_gates_resolved: 0 }, sources: [{ source_rule_id: "mrs:a", source_id: "a", source_name: "A", static_gate_resolved: false, remaining_static_blockers: ["component:critical-rate:expected-value-model-required"] }] });
    writeJson(files.workbench, {
      game_build: "1",
      summary: { blocker_obligations: 1, source_investigations_avoided_if_proved_by_group: 0 },
      model_groups: [
        {
          model_key: "expected-value:critical-rate", model_family: "expected-value", component_key: "critical-rate",
          proof_contract: "prove", registry_only_proof_route: false, source_count: 1, obligation_count: 1,
          source_rule_ids: ["mrs:a"], obligation_ids: ["mrs:a#0"], component_evidence_counts: { "exact-source-rule": 1 },
          manual_component_binding_obligations: 0, runtime_selector_obligations: 0,
          proof_receipts: [{ proof_id: "test-critical-rate", state: "exact-current-build-offline-formula-proven", model_keys: ["expected-value:critical-rate"], still_required_runtime_gates: ["counterfactual-projection"] }],
        },
        {
          model_key: "runtime-input:test-family", model_family: "runtime-input", component_key: "test-family",
          proof_contract: "preserve", registry_only_proof_route: true, source_count: 0, obligation_count: 0,
          source_rule_ids: [], obligation_ids: [], component_evidence_counts: {}, manual_component_binding_obligations: 0,
          runtime_selector_obligations: 0,
          proof_receipts: [{ proof_id: "test-runtime-family", state: CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE, model_keys: ["runtime-input:test-family"], still_required_runtime_gates: ["provider", "projection", "conservation"] }],
        },
      ],
      obligations: [{ obligation_id: "mrs:a#0", source_rule_id: "mrs:a" }],
    });
    writeJson(files.carryForward, { build_id: "1", proofs: [{ effect_id: 1, name: "historic", current_build_runtime_enabled: false }] });
    writeJson(files.protocolStatus, {
      schema_version: 1,
      generated_by: "tools/bpsr-protocol-pack-status.mjs",
      game_build: "1",
      status: "blocked",
      audit: { promotion_ready: false },
      promoted_pack: {
        present: false,
        build_matches: false,
        byte_identical_to_candidate: false,
      },
      blockers: ["matching-build route remains unproven"],
    });
    writeJson(files.runtimeEffectComponentRoutingProof, {
      schema_version: 1,
      generated_by: "self-test",
      game_build: "1",
      effect_routes: [
        {
          effect_id: "3003012",
          route_class: "mixed-external-and-non-outgoing",
          runtime_credit_candidate: true,
          proven_no_outgoing_attribution: false,
          component_counts: { total: 2, external_candidate: 1, proven_non_outgoing: 1, unresolved: 0 },
          source_rule_ids: ["mrs:test"],
          components: [
            {
              source_rule_id: "mrs:test",
              component_key: "target-vulnerability",
              label: "Target vulnerability",
              transfer_eligibility: "external-recipient-candidate",
              formula_replay_status: "blocked-formula-placement-unproven",
              value_resolution: "single",
              values: [{ raw_text: "10%", unit: "percent", value: 10, decimal_value: 0.1, formula_amount: true }],
            },
            {
              source_rule_id: "mrs:test",
              component_key: "attack-reduction",
              transfer_eligibility: "non-outgoing-context",
              formula_replay_status: "not-outgoing-rdps",
              value_resolution: "single",
              values: [{ raw_text: "20%", unit: "percent", value: 20, decimal_value: 0.2, formula_amount: true }],
            },
          ],
        },
        {
          effect_id: "3003014",
          route_class: "proven-non-outgoing-context",
          runtime_credit_candidate: false,
          proven_no_outgoing_attribution: true,
          component_counts: { total: 1, external_candidate: 0, proven_non_outgoing: 1, unresolved: 0 },
          source_rule_ids: ["mrs:test"],
          components: [{
            transfer_eligibility: "non-outgoing-context",
            formula_replay_status: "not-outgoing-rdps",
          }],
        },
      ],
      components_without_runtime_effect_binding: [],
    });
    const partySkillFixture = {
      schema_version: 2,
      generated_by: "tools/bpsr-party-skill-static-closure.mjs",
      game_build: "1",
      policy: {
        exact_numeric_skill_effect_buff_ids_and_build_are_authoritative: true,
        localized_names_and_descriptions_are_discovery_evidence_only: true,
        remote_player_cast_packets_required: false,
        remote_player_cast_packets_treated_as_zero: false,
        remote_player_cast_packets_synthesized: false,
        unresolved_skill_to_buff_edges_preserved: true,
        reviewed_candidate_links_are_exact_runtime_edges: false,
        provider_rdps_credit_allowed: false,
      },
      summary: {
        skill_candidates: 1,
        rogue_party_entry_candidates: 1,
        rdps_relevant_skill_candidates: 1,
        rdps_relevant_rogue_party_entry_candidates: 1,
        provider_rdps_credit_allowed_rows: 0,
        hidden_omissions: 0,
      },
      runtime_decision: {
        provider_rdps_credit_allowed: false,
        runtime_catalog_promotion_allowed: false,
        ui_rdps_display_allowed: false,
        ordinary_damage_totals_unchanged: true,
      },
      skill_candidates: [{
        skill_id: 10,
        localized_name_evidence: "Party Test",
        support_categories: ["party-offensive-stat"],
        presentation_state: "described-row-evidence-retained",
        skill_to_buff_graph_state: "unresolved-no-exact-skill-to-buff-edge-in-selected-static-tables",
        exact_reviewed_buff_or_status_ids: [],
        reviewed_candidate_skill_to_buff_links: [{
          buff_id: 20,
          exact_skill_to_buff_edge_proven: false,
          runtime_attribution_enabled: false,
        }],
        exact_build_party_talent_route_evidence: [{
          talent_id: 30,
          transformation_opcode_semantics_authoritative: false,
          runtime_formula_authority: false,
        }],
        proof_obligations: ["provider-ownership", "matching-build-canonical-conservation-replay"],
        rdps_relevant_candidate: true,
        runtime_formula_authority: false,
        provider_rdps_credit_allowed: false,
      }],
      rogue_party_entry_candidates: [{
        entry_id: 40,
        localized_name_evidence: "Team Entry Test",
        support_categories: ["party-offensive-stat"],
        exact_root_buff_id: 50,
        exact_entry_to_root_buff_edge: { target_present: true },
        candidate_child_buff_family: [{ buff_id: 51, exact_runtime_edge_proven: false }],
        proof_obligations: ["provider-ownership", "matching-build-canonical-conservation-replay"],
        rdps_relevant_candidate: true,
        runtime_formula_authority: false,
        provider_rdps_credit_allowed: false,
      }],
    };
    partySkillFixture.content_sha256 = contentHash(partySkillFixture);
    writeJson(files.partySkillStaticClosure, partySkillFixture);
    const emptyPartyDamageRelation = {
      event_count: 0,
      amount: 0,
      ability_ids: [],
      damage_source_actor_ids: [],
      damage_target_actor_ids: [],
    };
    const emptyPartyIdentityEvidence = {
      source_status_events: 0,
      source_player_identity_events: 0,
      source_non_player_identity_events: 0,
      source_identity_unresolved_events: 0,
      affected_entity_status_events: 0,
      affected_entity_player_identity_events: 0,
      affected_entity_non_player_identity_events: 0,
      affected_entity_identity_unresolved_events: 0,
      self_source_affected_status_events: 0,
      external_source_affected_status_events: 0,
      external_status_events_with_both_player_identities: 0,
      external_status_events_with_unresolved_identity: 0,
      external_status_events_with_both_in_observed_party_roster: 0,
      external_status_events_with_roster_evidence_but_lifecycle_coverage_open: 0,
      external_status_events_with_matching_last_observed_team_id: 0,
      external_status_events_with_mismatching_last_observed_team_ids: 0,
      external_status_events_with_unresolved_last_observed_team_id: 0,
      external_status_events_with_team_id_evidence_but_protocol_coverage_open: 0,
      matching_last_observed_team_ids: [],
      party_membership_proven_status_events: 0,
      party_membership_unproven_status_events: 0,
      source_actor_kinds: [],
      affected_entity_actor_kinds: [],
      source_character_ids: [],
      affected_entity_character_ids: [],
      source_class_ids: [],
      affected_entity_class_ids: [],
    };
    writeJson(files.partyEffectWindowAudit, {
      schema_version: 8,
      generated_by: "rlogs-bpsr-party-effect-window-audit",
      game_build: "1",
      policy: {
        exact_numeric_effect_ids_and_build_are_authoritative: true,
        localized_names_are_runtime_keys: false,
        remote_player_cast_packets_required: false,
        remote_player_cast_packets_treated_as_zero: false,
        remote_player_cast_packets_synthesized: false,
        status_rows_without_provider_are_preserved: true,
        actor_identity_is_event_time_canonical_evidence_only: true,
        player_identity_is_party_membership_authority: false,
        explicit_party_roster_evidence_consumed: true,
        party_roster_lifecycle_route_coverage_proven: false,
        exact_build_team_id_attribute_evidence_consumed: true,
        team_attribute_interpretation_build: "1",
        team_id_attribute_id: 194,
        team_member_count_attribute_id: 195,
        team_attribute_protocol_event_coverage_proven: false,
        matching_last_observed_team_ids_grant_party_membership_authority: false,
        fight_source_enum_build: "1",
        fight_source_type_identity_exact_build_gated: true,
        packet_origin_edges_are_skill_to_buff_edges: false,
        packet_origin_edges_are_provider_ownership_authority: false,
        packet_origin_edges_are_formula_authority: false,
        damage_links_preserve_affected_entity_as_actor_and_as_target: true,
        affected_entity_is_assumed_friendly: false,
        affected_entity_is_assumed_enemy: false,
        status_source_to_effect_target_lifecycle_is_preserved: true,
        status_source_is_provider_ownership_authority: false,
        effect_target_role_is_allegiance_neutral: true,
        effect_target_damage_actor_and_damage_target_edges_are_separate: true,
        damage_action_edges_preserve_actor_ability_and_target: true,
        damage_action_edges_are_causal_or_formula_authority: false,
        timeline_presence_is_formula_authority: false,
        provider_rdps_credit_authorized: false,
        runtime_promotion_allowed: false,
        ui_display_allowed: false,
      },
      inputs: {
        party_closure_bytes: statSync(files.partySkillStaticClosure).size,
        party_closure_sha256: hashFile(files.partySkillStaticClosure),
        rlogs: [{
          path: "capture.rlog",
          bytes: 1,
          sha256: "a".repeat(64),
        }],
      },
      summary: {
        rlogs_verified: 1,
        canonical_events: 4,
        damage_events: 1,
        cast_events_observed: 0,
        remote_cast_rows_synthesized: 0,
        party_roster_full_snapshot_events: 0,
        party_roster_members_observed_events: 0,
        party_roster_member_left_events: 0,
        party_roster_dissolved_events: 0,
        team_id_attribute_events: 0,
        team_id_attribute_positive_values: 0,
        team_id_attribute_clear_values: 0,
        team_id_attribute_malformed_values: 0,
        team_id_attribute_malformed_examples: [],
        team_member_count_attribute_events: 0,
        party_effects_in_frontier: 3,
        party_effects_observed: 1,
        party_status_events: 2,
        party_status_events_without_source: 0,
        windows: 1,
        windows_with_affected_entity_damage_actions: 1,
        windows_with_damage_actions_targeting_affected_entity: 0,
        window_damage_action_edges: 1,
        window_damage_action_actor_edges: 1,
        window_damage_action_target_edges: 0,
        provider_rdps_credit_authorized_effects: 0,
      },
      effects: [
        {
          effect_id: 20,
          exact_static_edge: false,
          reviewed_candidate_edge: true,
          source_skill_ids: [10],
          source_entry_ids: [],
          support_categories: ["party-offensive-stat"],
          status_events: 2,
          status_events_with_source: 2,
          status_events_without_source: 0,
          unique_source_actor_ids: ["1"],
          unique_affected_entity_actor_ids: ["2"],
          unique_origin_pairs: [],
          observed_origin_edges: [],
          lifecycle_counts: { applied: 1, removed: 1 },
          reported_duration_millis: [10000],
          reported_status_levels: [30],
          reported_stacks: [1],
          reported_counts: [-1],
          windows_closed: 1,
          windows_open_at_log_end: 0,
          orphan_lifecycle_windows: 0,
          identity_evidence: {
            ...emptyPartyIdentityEvidence,
            source_status_events: 2,
            source_player_identity_events: 2,
            affected_entity_status_events: 2,
            affected_entity_player_identity_events: 2,
            external_source_affected_status_events: 2,
            external_status_events_with_both_player_identities: 2,
            external_status_events_with_unresolved_last_observed_team_id: 2,
            party_membership_unproven_status_events: 2,
            source_actor_kinds: ["player"],
            affected_entity_actor_kinds: ["player"],
          },
          affected_entity_damage_actions: {
            event_count: 1,
            amount: 10,
            ability_ids: [100],
            damage_source_actor_ids: ["2"],
            damage_target_actor_ids: ["3"],
          },
          damage_actions_targeting_affected_entity: emptyPartyDamageRelation,
          provider_rdps_credit_authorized: false,
        },
        ...[50, 51].map((effectId) => ({
          effect_id: effectId,
          exact_static_edge: effectId === 50,
          reviewed_candidate_edge: effectId === 51,
          source_skill_ids: [],
          source_entry_ids: [40],
          support_categories: ["party-offensive-stat"],
          status_events: 0,
          status_events_with_source: 0,
          status_events_without_source: 0,
          unique_source_actor_ids: [],
          unique_affected_entity_actor_ids: [],
          unique_origin_pairs: [],
          observed_origin_edges: [],
          lifecycle_counts: {},
          reported_duration_millis: [],
          reported_status_levels: [],
          reported_stacks: [],
          reported_counts: [],
          windows_closed: 0,
          windows_open_at_log_end: 0,
          orphan_lifecycle_windows: 0,
          identity_evidence: emptyPartyIdentityEvidence,
          affected_entity_damage_actions: emptyPartyDamageRelation,
          damage_actions_targeting_affected_entity: emptyPartyDamageRelation,
          provider_rdps_credit_authorized: false,
        })),
      ],
      windows: [{
        effect_id: 20,
        affected_entity_actor_id: "2",
        affected_entity_uuid: "22",
        effect_target_actor_id: "2",
        effect_target_entity_uuid: "22",
        affected_entity_damage_actions: {
          event_count: 1,
          amount: 10,
          ability_ids: [100],
          damage_source_actor_ids: ["2"],
          damage_target_actor_ids: ["3"],
        },
        damage_actions_targeting_affected_entity: emptyPartyDamageRelation,
        damage_action_edges: [{
          role: "effect_target_is_damage_actor",
          damage_source_actor_id: "2",
          damage_source_entity_uuid: "22",
          direct_damage_source_actor_id: null,
          direct_damage_source_entity_uuid: null,
          ability_id: 100,
          damage_target_actor_id: "3",
          damage_target_entity_uuid: "33",
          event_count: 1,
          amount: 10,
          first_sequence: 2,
          last_sequence: 2,
          first_observed_micros: 20,
          last_observed_micros: 20,
          samples: [{
            sequence: 2,
            observed_micros: 20,
            amount: 10,
            actual_amount: null,
            hit_event_id: null,
            skill_effect_uuid: null,
          }],
          causal_attribution_authorized: false,
          provider_rdps_credit_authorized: false,
        }],
      }],
    });
    writeJson(files.runtimeAttributionEvidence, {
      runtime_rule_build: "1",
      relationship_catalog: [
        {
          deployment_id: "global", client_build: "1", protocol_pack_digest: "sha256:test",
          effect_id: 55228, affected_damage_id: 2203291, session_count: 1,
          damage_event_count: 1, observed_damage: "100", exact_integer_delta: "10",
          exact_rational_deltas: [], damage_context_complete: true,
          proof_status: "packet_replay_proven_exact_target",
        },
        {
          deployment_id: "global", client_build: "1", protocol_pack_digest: "sha256:test",
          effect_id: 3003052, affected_damage_id: 1222, session_count: 1,
          damage_event_count: 1, observed_damage: "100", exact_integer_delta: "0",
          exact_rational_deltas: [{ numerator: "3", denominator: "2", contribution_count: 1 }],
          damage_context_complete: true, proof_status: "packet_replay_proven_exact_target",
        },
        {
          deployment_id: "global", client_build: "1", protocol_pack_digest: "sha256:test",
          effect_id: 3003052, affected_damage_id: 1223, session_count: 1,
          damage_event_count: 1, observed_damage: "100", exact_integer_delta: "0",
          exact_rational_deltas: [{ numerator: "5", denominator: "4", contribution_count: 1 }],
          damage_context_complete: true, proof_status: "packet_replay_proven_exact_target",
        },
      ],
      reports: [{
        source_path: "fixture.rlog", session_id: "session-1", deployment_id: "global",
        client_build: "1", runtime_target_match: true, conserved: true,
        emitted_contribution_events_by_effect: { "55228": 1, "3003052": 2 },
        summary: {
          effects: [{ effect_id: 55228, provider_actor_id: 1, recipient_actor_id: 2, amount: 10 }],
          rational_effects: [{ effect_id: 3003052, provider_actor_id: 1, recipient_actor_id: 2, numerator: "11", denominator: "4" }],
          attributed_damage_event_count: 1, attributed_bonus_damage: 10, missing_source_status_count: 0,
        },
        influence_relationships: [
          {
            effect_id: 55228, provider_actor_id: "1", provider_entity_uuid: "11",
            recipient_actor_id: "2", recipient_entity_uuid: "22", affected_damage_id: 2203291,
            damage_source_actor_id: "2", damage_source_entity_uuid: "22",
            target_actor_id: "3", target_entity_uuid: "33", damage_event_count: 1,
            observed_damage: "100", exact_integer_delta: "10", exact_rational_deltas: [],
            damage_context_complete: true,
          },
          {
            effect_id: 3003052, provider_actor_id: "1", provider_entity_uuid: "11",
            recipient_actor_id: "2", recipient_entity_uuid: "22", affected_damage_id: 1222,
            damage_source_actor_id: "2", damage_source_entity_uuid: "22",
            target_actor_id: "3", target_entity_uuid: "33", damage_event_count: 1,
            observed_damage: "100", exact_integer_delta: "0",
            exact_rational_deltas: [{ numerator: "3", denominator: "2", contribution_count: 1 }],
            damage_context_complete: true,
          },
          {
            effect_id: 3003052, provider_actor_id: "1", provider_entity_uuid: "11",
            recipient_actor_id: "2", recipient_entity_uuid: "22", affected_damage_id: 1223,
            damage_source_actor_id: "2", damage_source_entity_uuid: "22",
            target_actor_id: "4", target_entity_uuid: "44", damage_event_count: 1,
            observed_damage: "100", exact_integer_delta: "0",
            exact_rational_deltas: [{ numerator: "5", denominator: "4", contribution_count: 1 }],
            damage_context_complete: true,
          },
        ],
        target_vulnerability_audit_gates: { multiple_external_active_providers: 2 },
      }],
    });
    writeJson(files.counterfactualFrontier, {
      schema_version: 3,
      generated_by: "rlogs-bpsr-status-effect-counterfactual-proof",
      game_build: "1",
      policy: {
        runtime_authority: false,
        formula_authority: false,
        unresolved_evidence_is_hidden: false,
      },
      input: {
        path: "cohort.json",
        bytes: 1,
        sha256: `sha256:${"0".repeat(64)}`,
        source_inputs: ["capture.rlog"],
      },
      summary: {
        distinct_effect_loci: 1,
        exact_controlled_groups: 1,
        exact_divergent_output_groups: 1,
      },
      effects: [{
        locus: "target",
        effect_id: 2206241,
        observation: { observed_status_states: 2, observed_samples: 2 },
        exact_recorded_inputs: {
          controlled_groups: 1,
          divergent_output_groups: 1,
          amount_differences: [{ value: 385, comparisons: 1 }],
          divergent_provider_relationship_groups: [{ relationship: "third_party", groups: 1, sample_comparisons: 1 }],
          divergent_examples: [],
        },
        target_current_hp_excluded_diagnostic: {
          controlled_groups: 1,
          divergent_output_groups: 1,
        },
        variants: [{
          status: { effect_id: 2206241, source_entity_uuid: 3, stacks: 1, level: 1, origin_source_type_id: 1, origin_source_config_id: 2206240 },
          exact_recorded_inputs: { controlled_groups: 1, divergent_output_groups: 1 },
          target_current_hp_excluded_diagnostic: { controlled_groups: 1, divergent_output_groups: 1 },
        }],
      }],
    });
    writeJson(files.counterfactualRollup, {
      schema_version: 1,
      generated_by: "reviewed-status-effect-counterfactual-rollup",
      game_build: "1",
      effect_id: 2110092,
      policy: {
        runtime_authority: false,
        formula_authority: false,
        provider_rdps_credit_allowed: false,
        cross_session_pairing_allowed: false,
        unresolved_evidence_is_preserved: true,
      },
      summary: {
        matching_capture_runs: 2,
        formula_damage_samples: 20,
        exact_controlled_groups: 0,
        exact_divergent_output_groups: 0,
      },
      loci: [{
        locus: "target",
        observed_samples: 10,
        exact: {
          present_groups: 10,
          absent_status_state_unobserved_groups: 9,
          absent_identity_group_unobserved_groups: 1,
          controlled_groups: 0,
          sample_comparisons: 0,
          divergent_output_groups: 0,
        },
        target_current_hp_excluded_diagnostic: {
          present_groups: 10,
          absent_status_state_unobserved_groups: 9,
          absent_identity_group_unobserved_groups: 1,
          controlled_groups: 0,
          sample_comparisons: 0,
          divergent_output_groups: 0,
        },
      }],
      runs: [{
        run: "capture",
        rlog: {
          path: "capture.rlog",
          bytes: 1,
          sha256: `sha256:${"a".repeat(64)}`,
        },
      }],
      status: "matching-build-observed-counterfactual-unproven",
      blockers: [
        "no same-session exact one-status-removed comparison was observed",
        "provider entity ownership to a player is unproven",
      ],
    });
    writeJson(files.providerOwnershipProof, {
      schema_version: 2,
      tool: "rlogs-bpsr-status-effect-provider-ownership-proof",
      game_build: "1",
      policy: {
        scope: "provider_ownership_only",
        exact_numeric_effect_ids_authoritative: true,
        exact_input_build_authoritative: true,
        localized_names_are_evidence_only: true,
        actor_kind_or_packet_proven_ancestry_required_for_player_ownership: true,
        future_actor_snapshots_may_backfill_prior_status_events: false,
        unknown_and_unresolved_events_preserved: true,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
      },
      inputs: [{
        path: "capture.rlog",
        bytes: 1,
        sha256: `sha256:${"a".repeat(64)}`,
        session_id: "capture",
        game_build: "1",
      }],
      summary: { selected_status_events: 3 },
      effects: [20, 2110092].map((effectId) => ({
        effect_id: effectId,
        status_events: effectId === 20 ? 2 : 1,
        resolution_counts: { direct_player: effectId === 20 ? 2 : 1 },
        unique_source_entities: 1,
        proven_player_character_ids: [],
        player_actor_ownership_proven_for_every_sourced_event: true,
        status_events_with_stable_player_character_id: 0,
        stable_player_character_id_proven_for_every_sourced_event: false,
        formula_authority: false,
        runtime_authority: false,
      })),
      resolutions: [20, 2110092].map((effectId) => ({
        effect_id: effectId,
        class: "direct_player",
        source: { actor_id: 1, entity_uuid: 11, kind: "player" },
        status_events: effectId === 20 ? 2 : 1,
      })),
    });
    build({
      build: "1",
      ...files,
      providerOwnershipProofs: [files.providerOwnershipProof],
    });
    const report = verify(files.output);
    if (report.summary.raw_frontier_work_items !== 1 || report.summary.audited_obligations !== 2 ||
      report.summary.manifest_indexed_obligations !== 2 || report.summary.shared_formula_models !== 2 ||
      report.summary.registry_only_proof_route_models !== 1) {
      throw new Error("Self-test component-fanout count failure");
    }
    if (report.schema_version !== PROOF_CLOSURE_SCHEMA_VERSION ||
      report.production_readiness.protocol_pack_status !== "blocked" ||
      report.production_readiness.runtime_promotion_allowed ||
      report.production_readiness.production_runtime_ready ||
      report.summary.runtime_promotable_obligations !== 0 ||
      report.summary.packet_observed_runtime_production_credit_allowed_effects !== 0 ||
      report.packet_observed_runtime_effect_results.some((effect) => effect.production_runtime_credit_allowed !== false)) {
      throw new Error("Self-test did not fail closed behind the protocol-pack promotion gate");
    }
    if (report.summary.party_skill_static_frontier_results !== 1 ||
      report.summary.party_rogue_entry_static_frontier_results !== 1 ||
      report.summary.party_skill_static_frontier_complete !== false ||
      report.party_skill_static_frontier.packet_obligations_fabricated !== 0 ||
      !report.production_readiness.blockers.includes(
        "party-skill-open:10:formula-scope-or-runtime-proof",
      ) ||
      !report.production_readiness.blockers.includes(
        "party-entry-open:40:formula-scope-or-runtime-proof",
      )) {
      throw new Error("Self-test did not preserve the fail-closed party-skill static frontier");
    }
    if (report.summary.party_effect_window_frontier_results !== 3 ||
      report.summary.party_effect_window_observed_effects !== 1 ||
      report.summary.party_effect_window_provider_ownership_proven_effects !== 1 ||
      report.summary.party_effect_window_frontier_complete !== false ||
      report.party_effect_window_frontier.affected_entity_allegiance_assumed !== false ||
      report.party_effect_window_frontier.remote_cast_rows_synthesized !== 0 ||
      report.party_effect_window_frontier.effect_results[0]
        .affected_entity_damage_actions.event_count !== 1 ||
      report.party_effect_window_frontier.effect_results[0]
        .damage_actions_targeting_affected_entity.event_count !== 0 ||
      report.party_effect_window_frontier.window_damage_action_edges !== 1 ||
      report.party_effect_window_frontier.window_damage_action_actor_edges !== 1 ||
      report.party_effect_window_frontier.window_damage_action_target_edges !== 0 ||
      report.party_effect_window_frontier.effect_results[0]
        .damage_action_edge_summary.effect_target_as_damage_actor_edges !== 1 ||
      report.party_effect_window_frontier.effect_results[0]
        .damage_action_edge_summary.effect_target_as_damage_target_edges !== 0 ||
      report.party_effect_window_frontier.effect_results[0]
        .provider_ownership_proven_for_every_sourced_status_event !== true ||
      report.party_effect_window_frontier.effect_results[0]
        .provider_ownership_proven_for_every_status_event !== true ||
      report.party_effect_window_frontier.effect_results[0]
        .blockers.includes("provider-ownership-for-every-status-event-open") ||
      report.party_effect_window_frontier.effect_results[0]
        .affected_entity_identity_proven_for_every_status_event !== true ||
      report.party_effect_window_frontier.effect_results[0]
        .affected_entity_player_identity_proven_for_every_status_event !== true ||
      report.party_effect_window_frontier.effect_results[0]
        .external_affected_entity_party_membership_proven_for_every_status_event !== false ||
      report.party_effect_window_frontier.effect_results[0]
        .blockers.includes("event-time-party-membership-for-external-affected-entities-open") ||
      report.party_effect_window_frontier.effect_results[0]
        .affected_entity_role_proven !== true ||
      report.party_effect_window_frontier.effect_results[0]
        .affected_entity_role_resolution !== "damage-actor-allegiance-neutral" ||
      report.party_effect_window_frontier.effect_results[0]
        .affected_entity_role_requires_party_membership !== false ||
      !report.production_readiness.blockers.includes(
        "party-effect-window-open:20:formula-stacking-rounding-conservation",
      )) {
      throw new Error("Self-test did not preserve allegiance-neutral party-effect windows");
    }
    if (report.obligation_results[0].status !== "observed-external-scope-awaiting-static-model") {
      throw new Error("Self-test did not preserve strict blockers");
    }
    const enrichedObligation = report.obligation_results[0];
    if (enrichedObligation.gates.external_provider_scope !== true ||
      enrichedObligation.gates.lifecycle !== true ||
      enrichedObligation.gates.formula_inputs !== false ||
      enrichedObligation.evidence.exact_terminal_effect_enrichment?.effect_ids?.join(",") !== "3003012" ||
      enrichedObligation.evidence.status_states.applied !== 1 ||
      enrichedObligation.evidence.status_states.removed !== 1 ||
      enrichedObligation.evidence.eligible_external_provider_recipient_observations !== 1 ||
      enrichedObligation.blockers.includes("provider-recipient-evidence-missing") ||
      enrichedObligation.blockers.includes("lifecycle-incomplete-or-ambiguous") ||
      !enrichedObligation.blockers.includes("required-formula-input-snapshots-incomplete")) {
      throw new Error("Self-test did not join exact terminal effect lifecycle and provider evidence");
    }
    if (report.shared_model_results[0].status !== "shared-model-proof-received-runtime-open" ||
      report.shared_model_results[0].blockers.includes("shared-static-model-has-no-current-build-proof-receipt") ||
      report.summary.shared_formula_models_proof_received_runtime_open !== 2 ||
      report.summary.shared_formula_models_offline_proven_runtime_open !== 1 ||
      report.summary.shared_formula_models_canonical_runtime_input_route_proven_runtime_open !== 1) {
      throw new Error("Self-test did not preserve offline proof while leaving runtime closure open");
    }
    const registryOnlyRoute = report.shared_model_results.find((entry) => entry.model_key === "runtime-input:test-family");
    if (!registryOnlyRoute?.registry_only_proof_route || registryOnlyRoute.runtime_manifest_obligations !== 0 ||
      registryOnlyRoute.still_required_runtime_gates.join(",") !== "conservation,projection,provider" ||
      !registryOnlyRoute.blockers.includes("shared-proof-runtime-gate-open:provider")) {
      throw new Error("Self-test did not preserve the deferred registry-only proof route and its open gates");
    }
    if (report.obligation_results[0].correlation_match.kind !== "runtime-selector-rekey" ||
      report.obligation_results[0].correlation_match.aggregate_obligation_id !== "old-a") {
      throw new Error("Self-test did not preserve metadata-only rekey provenance");
    }
    if (report.obligation_results[1].status !== "no-candidate-evidence") throw new Error("Self-test candidate classification failure");
    const counterfactual = report.counterfactual_frontier_results?.find(
      (entry) => entry.effect_id === "2206241",
    );
    const counterfactualRollup = report.counterfactual_frontier_results?.find(
      (entry) => entry.effect_id === "2110092",
    );
    if (counterfactual?.effect_id !== "2206241" || counterfactual.locus !== "target" ||
      counterfactual.status !== "controlled-delta-observed-proof-open" ||
      counterfactual.formula_authority !== false || counterfactual.runtime_authority !== false ||
      counterfactualRollup?.status !== "no-controlled-counterfactual-pair" ||
      counterfactualRollup.rollup_provenance?.matching_capture_runs !== 2 ||
      counterfactualRollup.provider_ownership_evidence?.run_scoped_player_ownership_proven !== true ||
      counterfactualRollup.blockers.includes("provider entity ownership to a player is unproven") ||
      !counterfactualRollup.blockers.includes("stable-player-character-id-unproven-for-cross-run-join") ||
      report.summary.counterfactual_frontier_effect_loci !== 2 ||
      report.summary.counterfactual_frontier_run_scoped_player_provider_owned_effect_loci !== 1 ||
      report.summary.attribution_progress.current_build_counterfactual_exact_divergent_effect_loci !== 1) {
      throw new Error("Self-test did not preserve the non-authoritative counterfactual frontier");
    }
    const exactPromotion = report.packet_observed_runtime_effect_results.find((entry) => entry.effect_id === "55228");
    const rationalPromotion = report.packet_observed_runtime_effect_results.find((entry) => entry.effect_id === "3003052");
    if (report.summary.packet_observed_runtime_attribution_promoted_exact_subset !== 2 ||
      exactPromotion?.status !== "runtime-attribution-promoted-exact-subset" ||
      exactPromotion.evidence.exact_replay_damage_events !== 1 ||
      exactPromotion.evidence.exact_replay_attributed_bonus_damage !== "10" ||
      exactPromotion.evidence.exact_replay_attributed_bonus_damage_rational?.numerator !== "10" ||
      exactPromotion.evidence.exact_replay_attributed_bonus_damage_rational?.denominator !== "1" ||
      exactPromotion.evidence.ambiguous_provider_window_events !== 2) {
      throw new Error("Self-test did not promote the conserved exact runtime attribution subset");
    }
    if (rationalPromotion?.status !== "runtime-attribution-promoted-exact-subset" ||
      rationalPromotion.evidence.exact_replay_damage_events !== 2 ||
      rationalPromotion.evidence.exact_replay_attributed_bonus_damage !== undefined ||
      rationalPromotion.evidence.exact_replay_attributed_bonus_damage_rational?.numerator !== "11" ||
      rationalPromotion.evidence.exact_replay_attributed_bonus_damage_rational?.denominator !== "4" ||
      rationalPromotion.evidence.exact_replay_affected_damage_ids.join(",") !== "1222,1223") {
      throw new Error("Self-test did not promote grouped exact rational runtime attribution");
    }
    const declaredScalarOpen = report.packet_observed_runtime_effect_results.find((entry) => entry.effect_id === "3003012");
    if (declaredScalarOpen?.status !== "runtime-external-open" ||
      !declaredScalarOpen.blockers.includes("exact-counterfactual-formula-placement-unresolved") ||
      declaredScalarOpen.blockers.includes("runtime-applied-magnitude-unresolved") ||
      declaredScalarOpen.evidence.declared_component_scalar_resolved !== true ||
      declaredScalarOpen.evidence.declared_component_scalars?.[0]?.decimal_value !== 0.1 ||
      declaredScalarOpen.evidence.formula_placement_statuses?.[0] !== "blocked-formula-placement-unproven") {
      throw new Error("Self-test lost a known component scalar behind an unresolved formula-placement blocker");
    }
    const nonOutgoingEffect = report.packet_observed_runtime_effect_results.find((entry) => entry.effect_id === "3003014");
    if (nonOutgoingEffect?.status !== "packet-observed-non-outgoing-context" ||
      nonOutgoingEffect.blockers.length !== 0 ||
      report.summary.packet_observed_runtime_non_outgoing_context !== 1 ||
      report.summary.packet_observed_runtime_external_candidates !== 3) {
      throw new Error("Self-test did not exclude the proven non-outgoing sibling from runtime attribution");
    }
    const ambiguousObserved = {
      ...observedBase,
      domain: "runtime",
      subject_kind: "x",
      subject_id: "1",
      required_event_kinds: ["actor", "damage", "status", "formula_inputs"],
      selector_contract: JSON.stringify({ source_rule_ids: ["mrs:old"], effect_ids: [1] }),
    };
    const ambiguousIndex = buildAggregateObservationIndex([
      { ...ambiguousObserved, obligation_id: "old-a-1" },
      { ...ambiguousObserved, obligation_id: "old-a-2" },
    ]);
    let ambiguityRejected = false;
    try {
      resolveAggregateObservation(
        {
          obligation_id: "new-a",
          domain: "runtime",
          subject_kind: "x",
          subject_id: "1",
          required_event_kinds: ["actor", "damage", "status", "formula_inputs"],
          selectors: { source_rule_ids: ["mrs:new"], effect_ids: [1] },
        },
        ambiguousIndex,
        new Set(),
      );
    } catch (error) {
      ambiguityRejected = String(error.message).includes("is ambiguous across");
    }
    if (!ambiguityRejected) throw new Error("Self-test accepted an ambiguous runtime selector rekey");
    const missing = resolveAggregateObservation(
      {
        obligation_id: "new-component",
        domain: "runtime",
        subject_kind: "x",
        subject_id: "1",
        required_event_kinds: ["actor", "status"],
        selectors: { source_rule_ids: ["mrs:new-component"], effect_ids: [2] },
      },
      buildAggregateObservationIndex([]),
      new Set(),
    );
    if (missing.kind !== "manifest-new-no-observation" || missing.observed.coverage_state !== "no-candidate-evidence") {
      throw new Error("Self-test did not preserve a new manifest obligation as explicit no-evidence");
    }
    console.log("bpsr-rdps-proof-closure self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function emptyAggregate(build, manifest) {
  return {
    manifest_game_build: String(build),
    observed_game_builds: [],
    provisional_build_mismatch: false,
    total_events: 0,
    summary: { candidate_event_coverage_complete: 0 },
    obligations: (manifest.obligations ?? []).map((obligation) => ({
      obligation_id: obligation.obligation_id,
      coverage_state: "no-candidate-evidence",
      observed_event_kinds: [],
      missing_event_kinds: obligation.required_event_kinds ?? [],
      direct_matches: 0,
      contextual_matches: 0,
      provider_recipient_observations: [],
      status_states: {},
      ambiguous_status_removals: 0,
      ambiguous_provider_window_damage_events: 0,
      recipient_window_damage_events: 0,
      target_window_damage_events: 0,
      single_provider_window_damage_events: 0,
      formula_input_snapshots: [],
      packet_damage_rows: [],
      projection_statuses: [],
      projected_provider_recipient_observations: [],
      projected_integer_events: 0,
      projected_rational_events: 0,
      projected_invalid_events: 0,
      projected_excluded_events: 0,
    })),
  };
}

function normalizeTransferGate(evidence) {
  const gate = evidence?.transfer_gate;
  return {
    kind: gate?.kind ?? evidence?.component_kind ?? null,
    attribution_route: gate?.attribution_route ?? null,
    authority: gate?.authority ?? null,
    runtime_credit_allowed: gate?.runtime_credit_allowed === true,
    required_current_build_evidence: uniqueSorted(gate?.required_current_build_evidence ?? []),
    forbidden_transfers: uniqueSorted(gate?.forbidden_transfers ?? []),
  };
}

function classifyTransferGate(kind) {
  if (EXTERNAL_TRANSFER_GATE_KINDS.has(kind)) return "externally-transferable";
  if (NONTRANSFER_GATE_KINDS.has(kind)) return "nontransfer";
  if (kind === "component-scoped-routing-only") return "component-scoped";
  if (UNRESOLVED_TRANSFER_GATE_KINDS.has(kind)) return "unresolved";
  if (kind === "non-outgoing-context") return "non-outgoing";
  return "missing";
}

function isClosedStatus(status) {
  return status === "proven-promotable" || status === "proven-zero-transfer-source-owned";
}

function isExternalObservation(entry) {
  const provider = entry?.provider_actor_id ?? entry?.provider ?? entry?.source_actor_id;
  const recipient = entry?.recipient_actor_id ?? entry?.recipient ?? entry?.target_actor_id;
  return provider !== undefined && recipient !== undefined && String(provider) !== String(recipient);
}

function isCompleteFormulaSnapshot(snapshot) {
  if (snapshot?.complete === true) return true;
  if (["complete", "resolved", "valid"].includes(String(snapshot?.state ?? "").toLowerCase())) return true;
  if (["complete", "resolved", "valid"].includes(String(snapshot?.status ?? "").toLowerCase())) return true;
  if (Array.isArray(snapshot?.missing_fields)) return snapshot.missing_fields.length === 0;
  if (Array.isArray(snapshot?.missing_inputs)) return snapshot.missing_inputs.length === 0;
  return false;
}

function projectedCount(value, fallback) {
  if (value === undefined || value === null || value === "") return Number(fallback ?? 0);
  const count = Number(value);
  if (!Number.isSafeInteger(count) || count < 0) throw new Error(`Invalid projected evidence count ${value}`);
  return count;
}

function formulaSnapshotCounts(observed) {
  const snapshots = observed?.formula_input_snapshots ?? [];
  const total = projectedCount(observed?.formula_input_snapshot_count, snapshots.length);
  const complete = projectedCount(
    observed?.complete_formula_input_snapshot_count,
    snapshots.filter(isCompleteFormulaSnapshot).length,
  );
  if (complete > total) throw new Error(`Complete formula snapshot count ${complete} exceeds total ${total}`);
  return { total, complete };
}

function indexWorkbenchModels(groups) {
  const result = new Map();
  for (const group of groups) {
    for (const sourceRuleId of group.source_rule_ids ?? []) {
      if (!result.has(sourceRuleId)) result.set(sourceRuleId, []);
      result.get(sourceRuleId).push(group.model_key);
    }
  }
  for (const [key, values] of result) result.set(key, uniqueSorted(values));
  return result;
}

function indexHistoricalProofs(proofs) {
  const result = new Map();
  for (const proof of proofs) {
    const ids = uniqueSorted([proof.effect_id, ...(proof.effect_ids ?? [])].filter((value) => value !== undefined), compareIdentifiers);
    for (const effectId of ids) {
      const key = String(effectId);
      if (!result.has(key)) result.set(key, []);
      result.get(key).push(`${proof.name ?? "historical proof"} (${proof.historical_packet_build_id ?? "unknown build"})`);
    }
  }
  return result;
}

function uniqueIndex(values, key, label) {
  const result = new Map();
  for (const value of values) {
    const id = value?.[key];
    if (id === undefined || id === null || id === "") throw new Error(`${label} is missing ${key}`);
    if (result.has(String(id))) throw new Error(`Duplicate ${label} ${id}`);
    result.set(String(id), value);
  }
  return result;
}

function uniqueBy(values, selector) {
  const result = new Map();
  for (const value of values) result.set(selector(value), value);
  return [...result.values()];
}

function countBy(values, selector) {
  const counts = {};
  for (const value of values) {
    const key = selector(value);
    counts[key] = (counts[key] ?? 0) + 1;
  }
  return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => compareText(left, right)));
}

function groupBy(values, keyOf) {
  const groups = new Map();
  for (const value of values) {
    const key = keyOf(value);
    const group = groups.get(key) ?? [];
    group.push(value);
    groups.set(key, group);
  }
  return groups;
}

function uniqueSorted(values, comparator = compareText) { return [...new Set(values.map((value) => String(value)))].sort(comparator); }
function uniqueSortedNumbers(values) {
  return [...new Set(values.map(Number).filter((value) => Number.isSafeInteger(value) && value > 0))]
    .sort((left, right) => left - right);
}
function compareIdentifiers(left, right) { const a = Number(left); const b = Number(right); return Number.isSafeInteger(a) && Number.isSafeInteger(b) && a !== b ? a - b : compareText(left, right); }
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function requireBuild(actual, expected, label) { if (String(actual) !== String(expected)) throw new Error(`${label} build ${actual} does not match ${expected}`); }
function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: hashFile(file) }; }
function isValidFileDescriptor(value) {
  return String(value?.path ?? "") !== "" && Number.isSafeInteger(Number(value?.bytes)) &&
    Number(value.bytes) > 0 && /^[0-9a-f]{64}$/.test(String(value?.sha256 ?? ""));
}
function sameFileContentIdentity(left, right) {
  return isValidFileDescriptor(left) && isValidFileDescriptor(right) &&
    Number(left.bytes) === Number(right.bytes) && String(left.sha256) === String(right.sha256);
}
function optionalFileDescriptor(file) {
  return existsSync(file)
    ? { ...fileDescriptor(file), present: true }
    : { path: file.replaceAll("\\", "/"), present: false, bytes: 0, sha256: null };
}
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(stableStringify(clone)); }
function lineTerminatedContentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(`${stableStringify(clone)}\n`); }
function orderedContentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(JSON.stringify(clone)); }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); if (["counterfactual-rollup", "provider-ownership-proof", "status-event-season-context-proof", "integer-transform-constraints", "component-scalar-proof", "support-effect-proof"].includes(key)) (parsed[key] ??= []).push(value); else parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function positiveInteger(value, label) { const parsed = Number(value); if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`--${label} must be a positive integer`); return parsed; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-rdps-proof-closure.mjs build --build <id> --manifest <json> --aggregate <json> --static-formula-evidence <json> --workbench <json> --carry-forward <json> --runtime-effect-component-routing-proof <json> --party-skill-static-closure <json> --party-effect-window-audit <json> [--runtime-attribution-evidence <json>] [--counterfactual-frontier <json>] [--counterfactual-rollup <json> ...] [--provider-ownership-proof <json> ...] [--status-event-season-context-proof <json> ...] [--season-state-mutation-proof <json>] [--party-haste-stacking-frontier <json>] [--action-speed-formula-proof <json> --party-haste-capacity-proof <json> --action-timing-ancestry-proof <json>] [--imagine-formula-proof <json>] [--imagine-status-attribute-tier-proof <json>] [--imagine-tier-window-counterfactual-inputs <json>] [--fatal-spiral-damage-stage-frontier <json>] [--target-vulnerability-formula-proof <json>] [--integer-transform-constraints <json> ...] [--component-scalar-proof <json> ...] [--support-effect-proof <json> ...] [--life-wave-trigger-proof <json>] [--life-wave-remote-inference-proof <json>] [--critical-damage-factor-interpretation-proof <json> (required with Inspiration schema 17)] [--protocol-status <json>] --output <json>\n  node tools/bpsr-rdps-proof-closure.mjs verify --input <json>\n  node tools/bpsr-rdps-proof-closure.mjs inspect --input <json> [--status <status>] [--model <model-key>] [--limit <count>]\n  node tools/bpsr-rdps-proof-closure.mjs self-test"); process.exit(exitCode); }
