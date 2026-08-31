#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
if (process.argv[2] === "self-test") {
  selfTest();
  process.exit(0);
}
const options = parseArgs(process.argv.slice(2));
const defaultReconciliation = "runtime-data/research/rdps/all-current-build-effects.reconciliation-20260820.json";
const defaultRuntime = "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-formula-runtime.v1.json";
const defaultClassification = "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-effect-classification.v1.json";
const defaultOutput = "runtime-data/research/rdps/all-current-build-effects.external-frontier-20260820.json";

if (options.verify) {
  verifyFrontier(readJson(resolvePath(options.verify), "external-effect frontier"));
  console.log(`verified ${resolvePath(options.verify)}`);
  process.exit(0);
}

const reconciliationPath = resolvePath(options.reconciliation || defaultReconciliation);
const runtimePath = resolvePath(options.runtime || defaultRuntime);
const classificationPath = resolvePath(options.classification || defaultClassification);
const outputPath = resolvePath(options.output || defaultOutput);
const reconciliation = readJson(reconciliationPath, "effect reconciliation");
const runtime = readJson(runtimePath, "rDPS runtime configuration");
const classification = readJson(classificationPath, "rDPS effect classification");
const buffTable = options.buffTable
  ? readJson(resolvePath(options.buffTable), "current-build BuffTable")
  : null;

if (String(reconciliation.game_build) !== String(runtime.game_build)) {
  throw new Error(`build mismatch: reconciliation=${reconciliation.game_build}, runtime=${runtime.game_build}`);
}
if (String(reconciliation.game_build) !== String(classification.game_build)) {
  throw new Error(`classification build mismatch: reconciliation=${reconciliation.game_build}, classification=${classification.game_build}`);
}
if (!reconciliation.policy?.ambiguous_and_unresolved_evidence_is_preserved
    || !runtime.policy?.canonical_events_retained
    || runtime.policy?.unresolved_events_hidden !== false) {
  throw new Error("input policy does not preserve unresolved canonical evidence");
}

const externalQueueStates = new Set([
  "external_formula_lifecycle_and_scalar_proven",
  "external_formula_lifecycle_missing",
  "external_lifecycle_formula_scope_unresolved",
  "external_lifecycle_scalar_unresolved",
]);
const externalEffects = reconciliation.effects
  .filter((effect) => externalQueueStates.has(effect.proof_queue))
  .sort((left, right) => left.effect_id - right.effect_id);
const runtimeBridges = buildRuntimeBridges(runtime);
const classificationsById = new Map((classification.effects || [])
  .map((entry) => [entry.effect_id, entry]));
const effects = externalEffects.map((effect) => classifyEffect(
  effect,
  runtimeBridges.get(effect.effect_id) || null,
  buffTable?.[String(effect.effect_id)] || null,
  classificationsById.get(effect.effect_id) || null,
));

const result = {
  schema_version: 1,
  game: reconciliation.game,
  game_build: String(reconciliation.game_build),
  generated_by: "tools/rdps-external-effect-frontier.mjs",
  policy: {
    every_externally_observed_candidate_is_retained: true,
    unresolved_damage_is_conserved_unattributed: true,
    no_rule_is_promoted_from_localization_text_alone: true,
    model_families_may_overlap: true,
    healing_and_mitigation_are_support_evidence_not_automatic_damage_rdps: true,
    cadence_is_not_replayed_as_a_flat_damage_multiplier: true,
    packet_provider_and_recipient_identity_outweighs_ambiguous_static_item_identity: true,
    historical_replay_uses_the_same_versioned_rules_as_live_reduction: true,
    missing_canonical_fields_remain_explicit_per_run_exactness_limits: true,
    zero_hidden_omissions: true,
  },
  inputs: {
    reconciliation: relativePath(reconciliationPath),
    runtime_configuration: relativePath(runtimePath),
    current_build_buff_table: options.buffTable ? relativePath(resolvePath(options.buffTable)) : null,
    effect_classification: relativePath(classificationPath),
  },
  summary: summarize(effects, reconciliation.effects.length, runtimeBridges),
  effects,
};

verifyFrontier(result);
writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`);
console.log(JSON.stringify({ output: outputPath, ...result.summary }, null, 2));

function classifyEffect(effect, runtimeBridge, buffRow = null, reviewedClassification = null) {
  const components = effect.formula_endpoint?.components || [];
  const lifecycle = effect.packet_lifecycle || {};
  const modelFamilies = classifyModelFamilies(effect, components, runtimeBridge, reviewedClassification);
  const offensiveFamilies = modelFamilies.filter((family) => isDamageRelevantFamily(family));
  const supportFamilies = modelFamilies.filter((family) => ["healing-support", "shield-mitigation-support"].includes(family));
  const primaryFamily = selectPrimaryFamily(modelFamilies, offensiveFamilies, supportFamilies);
  const lifecycleRoute = lifecycle.resolved_external_player_to_player_windows > 0
    ? "external-player-to-player"
    : lifecycle.resolved_player_to_monster_windows > 0
      ? "player-to-monster-target"
      : "not-yet-observed";
  const gates = buildProofGates(
    effect,
    runtimeBridge,
    offensiveFamilies,
    supportFamilies,
    lifecycleRoute,
    reviewedClassification,
  );
  const promotionState = selectPromotionState(
    effect,
    runtimeBridge,
    offensiveFamilies,
    supportFamilies,
    gates,
    reviewedClassification,
  );
  const packetOriginKindsByConfig = new Map((effect.packet_origins || [])
    .map((origin) => [origin.source_config_id, origin.source_kind]));
  const sourceCandidates = (effect.source_resolution?.candidates || []).map((source) => {
    const exactPacketOriginKind = source.source_id?.startsWith("buff-source:")
      ? packetOriginKindsByConfig.get(source.source_entity_id)
      : null;
    return {
      source_id: source.source_id,
      source_kind: exactPacketOriginKind || source.source_kind,
      source_type: exactPacketOriginKind ? `${exactPacketOriginKind}-origin` : source.source_type,
      source_name: source.source_name,
      source_entity_id: source.source_entity_id,
      runtime_detection: source.runtime_detection,
      owning_source_resolution_state: source.owning_source_resolution_state,
    };
  });
  const descriptionClauses = uniqueText(
    (effect.formula_endpoint?.components || [])
      .flatMap((component) => component.values || [])
      .flatMap((value) => [value.source_text, value.raw_text])
      .concat([buffRow?.Name, buffRow?.Desc, buffRow?.NameDesign]),
  );
  const mechanicContract = buildMechanicContract(effect, modelFamilies, descriptionClauses);
  return {
    effect_id: effect.effect_id,
    display_name: effect.display_name,
    current_proof_queue: effect.proof_queue,
    primary_model_family: primaryFamily,
    model_families: modelFamilies,
    damage_relevant_model_families: offensiveFamilies,
    support_model_families: supportFamilies,
    lifecycle_route: lifecycleRoute,
    packet_lifecycle: {
      status_events: lifecycle.status_events || 0,
      windows: lifecycle.window_count || 0,
      external_player_to_player_windows: lifecycle.resolved_external_player_to_player_windows || 0,
      player_to_monster_windows: lifecycle.resolved_player_to_monster_windows || 0,
      unresolved_cross_actor_windows: lifecycle.unresolved_cross_actor_windows || 0,
      minimum_stacks: lifecycle.minimum_stacks ?? null,
      maximum_stacks: lifecycle.maximum_stacks ?? null,
    },
    packet_origins: (effect.packet_origins || []).map((origin) => ({
      source_type_id: origin.source_type_id,
      source_kind: origin.source_kind,
      configured_source_table: origin.configured_source_table,
      source_config_id: origin.source_config_id,
      observation_count: origin.observation_count,
    })),
    source_resolution: {
      state: effect.source_resolution?.state || "unresolved",
      exact_per_packet_origin: Boolean(effect.source_resolution?.exact_per_packet_origin),
      exact_owning_source: Boolean(effect.source_resolution?.exact_owning_source),
      candidate_source_ids: effect.source_resolution?.candidate_source_ids || [],
      selected_grade_or_tier_proven: Boolean(effect.source_resolution?.selected_grade_or_tier_proven),
      candidates: sourceCandidates,
    },
    exact_description_clauses: descriptionClauses,
    current_build_status_row: buffRow ? {
      id: buffRow.Id,
      level: buffRow.Level,
      design_name: buffRow.NameDesign,
      localized_name: buffRow.Name,
      description: buffRow.Desc,
      tips_description_id: buffRow.TipsDescription,
      buff_type: buffRow.BuffType,
      visible: buffRow.Visible,
      repeat_add_rule: buffRow.RepeatAddRule || [],
      destroy_param: buffRow.DestroyParam || [],
      special_attributes: buffRow.SpecialAttr || [],
    } : null,
    reviewed_classification: reviewedClassification,
    mechanic_contract: mechanicContract,
    formula_evidence: summarizeFormulaEvidence(effect.formula_endpoint),
    runtime_bridge: runtimeBridge,
    proof_gates: gates,
    promotion_state: promotionState,
    current_damage_disposition: selectAccountingDisposition(
      promotionState,
      offensiveFamilies,
      supportFamilies,
      reviewedClassification,
    ),
    historical_replay: {
      supported_after_promotion: true,
      rule_source: "same-versioned-rule-pack-as-live-reducer",
      exact_only_when_required_canonical_fields_are_present: true,
      missing_fields_for_exact_replay: gates
        .filter((gate) => gate.state !== "proven" && gate.canonical_field)
        .map((gate) => gate.canonical_field),
    },
  };
}

function buildMechanicContract(effect, modelFamilies, descriptionClauses) {
  const components = (effect.formula_endpoint?.components || []).map((component) => ({
    component_key: component.component_key || null,
    effect_class: component.effect_class || null,
    direction: component.direction || null,
    stat: component.stat || null,
    contribution_scope: component.contribution_scope || null,
    value_scope: component.value_scope || null,
    formula_term_ids: [...(component.formula_term_ids || [])].sort(compareText),
    formula_zone_ids: [...(component.formula_zone_ids || [])].sort(compareText),
    values: (component.values || []).map((value) => ({
      scope: value.scope || null,
      key: value.key || null,
      unit: value.unit || null,
      value: value.value ?? null,
      decimal_value: value.decimal_value ?? null,
      raw_table_value: value.raw_table_value ?? null,
      tier: value.tier ?? null,
      tier_kind: value.tier_kind || null,
    })),
  }));
  const contract = {
    model_families: [...modelFamilies].sort(compareText),
    scope_kinds: [...(effect.formula_endpoint?.scope_kinds || [])].sort(compareText),
    stack_policy: effect.formula_endpoint?.stack_policy || null,
    value_resolution: effect.formula_endpoint?.value_resolution || null,
    components,
    description_clauses: descriptionClauses,
  };
  const operatorShape = {
    model_families: contract.model_families,
    scope_kinds: contract.scope_kinds,
    stack_policy: contract.stack_policy,
    components: components.map((component) => ({
      component_key: component.component_key,
      effect_class: component.effect_class,
      direction: component.direction,
      stat: component.stat,
      contribution_scope: component.contribution_scope,
      value_scope: component.value_scope,
      formula_term_ids: component.formula_term_ids,
      formula_zone_ids: component.formula_zone_ids,
      value_shapes: component.values.map((value) => ({
        scope: value.scope,
        key: value.key,
        unit: value.unit,
        tier_kind: value.tier_kind,
      })),
    })),
  };
  return {
    ...contract,
    reusable_contract_key: createHash("sha256")
      .update(JSON.stringify(operatorShape))
      .digest("hex"),
    contract_instance_key: createHash("sha256")
      .update(JSON.stringify(contract))
      .digest("hex"),
  };
}

function classifyModelFamilies(effect, components, runtimeBridge, reviewedClassification = null) {
  if (reviewedClassification?.review_state === "candidate"
      && reviewedClassification.contribution_kind === "state_scaling") {
    return ["state-scaled-damage"];
  }
  if (reviewedClassification?.review_state === "non_contributing") {
    if (reviewedClassification.contribution_kind === "healing_support") return ["healing-support"];
    if (reviewedClassification.contribution_kind === "internal_marker") return ["internal-marker"];
    if (reviewedClassification.contribution_kind === "self_only"
        || reviewedClassification.target_scope === "self_only") return ["self-only"];
    if (reviewedClassification.source_scope === "environment") return ["environmental"];
    if (reviewedClassification.contribution_kind === "mitigation") return ["shield-mitigation-support"];
    return ["reviewed-non-contributor"];
  }
  const families = new Set();
  // A damage-to-healing conversion can be shaped like a generic damage
  // modifier by the localization formula parser (for example, "heals ... for
  // 45% of the damage dealt"). Keep that component in formula_evidence, but do
  // not let it establish an offensive model family. It changes a support
  // outcome, not the triggering action's damage.
  const offensiveComponents = components.filter((component) => !isDamageToHealingConversion(component));
  const classes = new Set(offensiveComponents.map((component) => component.effect_class).filter(Boolean));
  const directions = new Set(offensiveComponents.map((component) => component.direction).filter(Boolean));
  const zones = new Set((effect.formula_endpoint?.formula_zone_ids || []).filter(Boolean));
  const stats = new Set(offensiveComponents.map((component) => component.stat).filter(Boolean));
  const keys = new Set(offensiveComponents.map((component) => component.component_key).filter(Boolean));
  const readiness = effect.formula_endpoint?.formula_readiness || "none";
  const defensive = hasDefensiveEvidence(effect, components);
  const healing = hasHealingEvidence(effect, components);

  if (healing) families.add("healing-support");
  if (defensive) families.add("shield-mitigation-support");
  if (!defensive && intersects(classes, ["damage-modifier", "skill-specific-damage-modifier", "elemental-damage", "final-damage"])) {
    families.add("direct-damage-multiplier");
  }
  if (!defensive && (classes.has("target-vulnerability")
      || (effect.packet_lifecycle?.resolved_player_to_monster_windows > 0
        && intersects(classes, ["damage-modifier", "target-mitigation"])))) {
    families.add("target-vulnerability");
  }
  if (classes.has("proc-damage") || ["packet-exact-produced-damage", "mixed-exact-and-replay"].includes(readiness)) {
    families.add("produced-proc-damage");
  }
  if (intersects(classes, ["critical-stat", "lucky-strike-chance"])
      || intersects(zones, ["critical", "luckyChance"])) {
    families.add("critical-lucky-probability");
  }
  if (intersects(classes, ["critical-damage-stat", "lucky-strike-damage"])
      || zones.has("luckyEnhancement")) {
    families.add("critical-lucky-magnitude");
  }
  if (intersects(classes, ["hit-timing", "cooldown-or-resource"])
      || directions.has("timing") || zones.has("timingCadence")) {
    families.add("cadence-timing");
  }
  if (classes.has("stat-conversion") || classes.has("mastery-stat")
      || directions.has("stat-conversion") || stats.has("mastery")) {
    families.add("stat-conversion");
  }
  if (!defensive && !healing && (classes.has("offense-stat") || zones.has("baseAttackTerm")
      || intersects(stats, ["attack", "physicalAttack", "magicAttack", "adaptivePrimary"]))) {
    families.add("offensive-stat");
  }
  if (classes.has("hit-count-model")) families.add("hit-count-model");
  if (classes.has("formula-input-dependency") && families.size === 0) families.add("formula-input-dependency");
  if (runtimeBridge?.model_families) {
    for (const family of runtimeBridge.model_families) families.add(family);
  }

  // A localized reduction label can only veto unsafe offensive promotion. It
  // never proves a positive attribution rule by itself.
  if (defensive) {
    families.delete("direct-damage-multiplier");
    families.delete("target-vulnerability");
  }
  if (families.size === 0 && keys.size === 0) families.add("unclassified-mechanic");
  else if (families.size === 0) families.add("unclassified-formula-effect");
  return [...families].sort(compareText);
}

function hasDefensiveEvidence(effect, components) {
  const name = String(effect.display_name || "").toLowerCase();
  if (/shield|resistance|mitigation|damage reduction|减免|护盾|防御/.test(name)) return true;
  return components.some((component) => [
    "shield-formula-input", "target-mitigation", "defense", "damage-reduction",
  ].includes(component.effect_class)
    || ["damage-dealt-reduction", "defense", "mitigation", "shield"].includes(component.direction));
}

function hasHealingEvidence(effect, components) {
  const name = String(effect.display_name || "").toLowerCase();
  return components.some((component) => component.effect_class === "healing-formula-input"
    || component.direction === "healing" || component.formula_zone_ids?.includes("healing")
    || isDamageToHealingConversion(component))
    || /healing|heal |hp recovery|恢复生命/.test(name);
}

function isDamageToHealingConversion(component) {
  return (component.values || []).some((value) => {
    const text = [value?.source_text, value?.raw_text]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    if (!text) return false;
    const healingOutcome = /\b(?:heal(?:s|ed|ing)?|restore(?:s|d|ing)?\s+(?:the\s+)?(?:target(?:'s)?\s+)?hp|hp\s+recovery)\b/.test(text);
    const damageBasis = /\b(?:damage\s+dealt|damage\s+caused|of\s+(?:the\s+)?damage|from\s+(?:the\s+)?damage)\b/.test(text);
    return healingOutcome && damageBasis;
  });
}

function isDamageRelevantFamily(family) {
  return [
    "direct-damage-multiplier", "target-vulnerability", "produced-proc-damage",
    "critical-lucky-probability", "critical-lucky-magnitude", "cadence-timing",
    "stat-conversion", "offensive-stat", "hit-count-model", "formula-input-dependency",
  ].includes(family);
}

function selectPrimaryFamily(families, offensive, support) {
  if (offensive.length > 1 || (offensive.length > 0 && support.length > 0)) return "mixed-model";
  if (offensive.length === 1) return offensive[0];
  if (support.length > 1) return "mixed-support";
  return support[0] || families[0] || "unclassified-mechanic";
}

function buildProofGates(
  effect,
  runtimeBridge,
  offensiveFamilies,
  supportFamilies,
  lifecycleRoute,
  reviewedClassification = null,
) {
  const runtimePromoted = runtimeBridge?.runtime_transfer_enabled === true;
  const ready = runtimeBridge?.runtime_transfer_enabled === true ||
    (effect.proof_queue === "external_formula_lifecycle_and_scalar_proven" &&
      runtimeBridge?.runtime_transfer_enabled !== false);
  const lifecycleProven = runtimePromoted || lifecycleRoute !== "not-yet-observed";
  const providerProven = runtimePromoted ||
    (lifecycleProven && effect.packet_lifecycle?.owner_resolved_provider_recipient_subdivision_available);
  const formulaPresent = runtimePromoted || Boolean(effect.formula_endpoint);
  const exactScalar = Boolean(effect.transfer_proof?.exact_scalar_available);
  const offensive = offensiveFamilies.length > 0;
  const supportOnly = !offensive && supportFamilies.length > 0;
  const reviewedNonContributor = reviewedClassification?.review_state === "non_contributing";
  const gates = [
    gate("packet-occurrence", runtimePromoted || (effect.packet_lifecycle?.status_events || 0) > 0,
      "status_effect_lifecycle"),
    gate("provider-recipient-identity", providerProven, "provider_and_recipient_entity_ids"),
    gate("active-window", lifecycleProven, "effect_apply_refresh_remove_timestamps"),
    reviewedNonContributor
      ? notApplicableGate("formula-identity")
      : gate("formula-identity", formulaPresent, "effect_formula_endpoint"),
    reviewedNonContributor
      ? notApplicableGate("exact-magnitude-or-vector")
      : gate("exact-magnitude-or-vector", exactScalar || runtimeBridge?.vector_state === "exact", "effect_scalar_or_attribute_vector"),
    offensive
      ? gate("damage-stage-counterfactual", ready, "damage_stage_inputs_at_event_time")
      : notApplicableGate("damage-stage-counterfactual"),
    offensive
      ? gate("exact-damage-conservation", ready, "party_damage_conservation_ledger")
      : notApplicableGate("exact-damage-conservation"),
  ];
  if (supportOnly) {
    gates.push(gate("support-outcome-model", runtimePromoted,
      "healing_shield_or_mitigation_outcome_timeline"));
    gates.push(gate("exact-support-conservation", runtimePromoted,
      "party_support_conservation_ledger"));
  }
  if (offensiveFamilies.includes("cadence-timing")) {
    gates.push(gate("cadence-counterfactual", runtimePromoted,
      "cast_and_action_opportunity_timeline"));
  }
  if (offensiveFamilies.some((family) => ["critical-lucky-probability", "critical-lucky-magnitude"].includes(family))) {
    gates.push(gate("critical-lucky-counterfactual", runtimePromoted,
      "critical_lucky_roll_and_damage_inputs"));
  }
  if (offensiveFamilies.includes("stat-conversion")) {
    gates.push(gate("class-spec-stat-transform", ready,
      "recipient_class_spec_and_stat_snapshot"));
  }
  if ((effect.packet_origins || []).length > 1) {
    gates.push(gate("per-origin-provider-split",
      runtimePromoted || Boolean(effect.source_resolution?.exact_per_packet_origin),
      "packet_configured_origin_per_window"));
  }
  return gates;
}

function gate(name, proven, canonicalField) {
  return { gate: name, state: proven ? "proven" : "required", canonical_field: canonicalField };
}

function notApplicableGate(name) {
  return { gate: name, state: "not-applicable", canonical_field: null };
}

function selectAccountingDisposition(
  promotionState,
  offensiveFamilies,
  supportFamilies,
  reviewedClassification = null,
) {
  if (reviewedClassification?.review_state === "non_contributing") {
    return "retained-reviewed-non-damage";
  }
  if (["production-promoted", "ready-for-damage-attribution"].includes(promotionState)) {
    return "versioned-rule-replay-enabled";
  }
  if (offensiveFamilies.length > 0) return "conserved-unattributed-until-promoted";
  if (supportFamilies.length > 0) return "retained-for-support-metric-attribution";
  return "retained-unclassified-until-proven";
}

function selectPromotionState(
  effect,
  runtimeBridge,
  offensiveFamilies,
  supportFamilies,
  gates,
  reviewedClassification = null,
) {
  if (reviewedClassification?.review_state === "non_contributing") {
    return "reviewed-non-contributor";
  }
  if (runtimeBridge?.runtime_transfer_enabled === true) return "production-promoted";
  if (effect.proof_queue === "external_formula_lifecycle_and_scalar_proven"
      && offensiveFamilies.length > 0 && gates.every((gate) => gate.state === "proven")) {
    return "ready-for-damage-attribution";
  }
  if (offensiveFamilies.length === 0 && supportFamilies.length > 0) return "support-metric-proof-queue";
  if (!effect.formula_endpoint) return "awaiting-effect-semantics";
  if ((effect.packet_lifecycle?.resolved_external_player_to_player_windows || 0) === 0
      && (effect.packet_lifecycle?.resolved_player_to_monster_windows || 0) === 0) {
    return "awaiting-external-lifecycle";
  }
  if (offensiveFamilies.length === 0) return "awaiting-model-classification";
  return "awaiting-counterfactual-model";
}

function summarizeFormulaEvidence(formula) {
  if (!formula) return null;
  return {
    key: formula.key,
    readiness: formula.formula_readiness,
    value_resolution: formula.value_resolution,
    formula_zone_ids: formula.formula_zone_ids || [],
    scope_kinds: formula.scope_kinds || [],
    stack_policy: formula.stack_policy,
    components: (formula.components || []).map((component) => ({
      component_key: component.component_key,
      effect_class: component.effect_class,
      direction: component.direction,
      stat: component.stat,
      value_scope: component.value_scope,
      formula_term_ids: component.formula_term_ids || [],
      formula_zone_ids: component.formula_zone_ids || [],
      values: component.values || [],
    })),
  };
}

function buildRuntimeBridges(runtime) {
  const bridges = new Map();
  for (const effectId of runtime.target_vulnerability?.runtime_transfer_effect_ids ?? []) {
    addBridge(bridges, effectId, "target_vulnerability", "exact", true,
      ["target-vulnerability"]);
  }
  addBridge(bridges, runtime.team_luck?.effect_id, "team_luck", "exact",
    runtime.team_luck?.critical_damage_runtime_transfer_enabled === true ||
      runtime.team_luck?.lucky_damage_runtime_transfer_enabled === true,
    ["critical-lucky-probability", "critical-lucky-magnitude"]);
  addBridge(bridges, runtime.functional_amp?.effect_id, "functional_amp", "exact",
    runtime.functional_amp?.attack_magic_runtime_transfer_enabled === true,
    ["offensive-stat"]);
  addBridge(bridges, runtime.mechanical_power?.effect_id, "mechanical_power", "exact",
    runtime.mechanical_power?.runtime_transfer_enabled === true, ["offensive-stat", "cadence-timing"]);
  addBridge(bridges, runtime.harmony_grace?.effect_id, "harmony_grace", "exact",
    runtime.harmony_grace?.runtime_transfer_enabled === true,
    ["stat-conversion"]);
  addBridge(bridges, runtime.inspire?.effect_id, "inspire", "exact",
    runtime.inspire?.runtime_transfer_enabled === true,
    ["cadence-timing"]);
  addBridge(bridges, runtime.stat_resonance?.effect_id, "stat_resonance", "exact",
    runtime.stat_resonance?.runtime_transfer_enabled === true, ["offensive-stat"]);
  addBridge(bridges, runtime.fiery_battle_will?.effect_id, "fiery_battle_will", "exact",
    runtime.fiery_battle_will?.runtime_transfer_enabled === true, ["offensive-stat"]);
  addBridge(bridges, runtime.encore?.effect_id, "encore", "exact",
    runtime.encore?.runtime_transfer_enabled === true && runtime.game_build === "24687926",
    ["produced-proc-damage"]);
  addBridge(bridges, runtime.thunderwind?.effect_id, "thunderwind", "partial", false,
    ["critical-lucky-probability", "critical-lucky-magnitude"]);
  addBridge(bridges, runtime.thunderwind?.child_effect_id, "thunderwind_child", "partial", false,
    ["critical-lucky-probability", "critical-lucky-magnitude"]);
  addBridge(bridges, runtime.inspiration?.effect_id, "inspiration", "exact",
    runtime.inspiration?.critical_chance_runtime_transfer_enabled === true ||
      runtime.inspiration?.lucky_chance_runtime_transfer_enabled === true,
    ["stat-conversion", "critical-lucky-probability"]);
  addBridge(bridges, runtime.inspiration?.full_bloom_effect_id, "inspiration_full_bloom", "exact",
    runtime.inspiration?.runtime_transfer_enabled === true, ["stat-conversion", "critical-lucky-probability"]);
  addBridge(bridges, runtime.highland_blood?.effect_id, "highland_blood", "exact",
    runtime.highland_blood?.runtime_transfer_enabled === true ||
      runtime.highland_blood?.remote_paired_output_runtime_transfer_enabled === true,
    ["direct-damage-multiplier"]);
  addBridge(bridges, runtime.critical_cold?.effect_id, "critical_cold", "exact",
    runtime.critical_cold?.runtime_transfer_enabled === true,
    ["critical-lucky-probability"]);
  return bridges;
}

function addBridge(bridges, effectId, section, vectorState, runtimeEnabled, modelFamilies) {
  if (!Number.isInteger(effectId)) return;
  bridges.set(effectId, {
    runtime_section: section,
    vector_state: vectorState,
    runtime_transfer_enabled: runtimeEnabled,
    model_families: modelFamilies,
    exact_vector_does_not_imply_exact_damage_counterfactual: true,
  });
}

function summarize(effects, observedEffects, runtimeBridges) {
  const primary = countBy(effects, (effect) => effect.primary_model_family);
  const promotion = countBy(effects, (effect) => effect.promotion_state);
  const queues = countBy(effects, (effect) => effect.current_proof_queue);
  const modelOverlap = {};
  for (const effect of effects) {
    for (const family of effect.model_families) modelOverlap[family] = (modelOverlap[family] || 0) + 1;
  }
  const productionEffectIds = [...runtimeBridges.entries()]
    .filter(([, bridge]) => bridge.runtime_transfer_enabled === true)
    .map(([effectId]) => effectId)
    .sort((left, right) => left - right);
  const contractClusters = countBy(
    effects,
    (effect) => effect.mechanic_contract.reusable_contract_key,
  );
  return {
    all_observed_effects: observedEffects,
    external_candidates: effects.length,
    external_candidates_retained: effects.length,
    ready_for_damage_attribution: promotion["ready-for-damage-attribution"] || 0,
    production_promoted_effect_count: productionEffectIds.length,
    production_promoted_effect_ids: productionEffectIds,
    production_promoted_observed_frontier_effect_count:
      promotion["production-promoted"] || 0,
    damage_relevant_candidates: effects.filter((effect) => effect.damage_relevant_model_families.length > 0).length,
    support_only_candidates: effects.filter((effect) =>
      effect.damage_relevant_model_families.length === 0 && effect.support_model_families.length > 0).length,
    unclassified_candidates: effects.filter((effect) =>
      effect.damage_relevant_model_families.length === 0 && effect.support_model_families.length === 0).length,
    conserved_unattributed_damage_candidates: effects.filter((effect) =>
      effect.current_damage_disposition === "conserved-unattributed-until-promoted").length,
    retained_support_candidates: effects.filter((effect) =>
      effect.current_damage_disposition === "retained-for-support-metric-attribution").length,
    retained_unclassified_candidates: effects.filter((effect) =>
      effect.current_damage_disposition === "retained-unclassified-until-proven").length,
    retained_reviewed_non_damage: effects.filter((effect) =>
      effect.current_damage_disposition === "retained-reviewed-non-damage").length,
    current_proof_queue_counts: queues,
    primary_model_family_counts: primary,
    overlapping_model_family_counts: Object.fromEntries(Object.entries(modelOverlap).sort(([a], [b]) => compareText(a, b))),
    promotion_state_counts: promotion,
    reusable_mechanic_contract_count: Object.keys(contractClusters).length,
    reusable_mechanic_contract_size_counts: countBy(
      Object.values(contractClusters),
      (count) => String(count),
    ),
    zero_hidden_omissions: effects.length > 0,
  };
}

function verifyFrontier(frontier) {
  if (!frontier.policy?.every_externally_observed_candidate_is_retained
      || !frontier.policy?.unresolved_damage_is_conserved_unattributed
      || !frontier.policy?.zero_hidden_omissions) {
    throw new Error("frontier safety policy is incomplete");
  }
  if (!Array.isArray(frontier.effects) || frontier.effects.length === 0) throw new Error("frontier has no effects");
  const ids = frontier.effects.map((effect) => effect.effect_id);
  if (new Set(ids).size !== ids.length) throw new Error("frontier contains duplicate effect IDs");
  if (frontier.summary.external_candidates !== frontier.effects.length
      || frontier.summary.external_candidates_retained !== frontier.effects.length) {
    throw new Error("frontier candidate conservation mismatch");
  }
  const primaryTotal = Object.values(frontier.summary.primary_model_family_counts || {})
    .reduce((sum, value) => sum + value, 0);
  if (primaryTotal !== frontier.effects.length) throw new Error("primary model family counts do not conserve effects");
  for (const effect of frontier.effects) {
    if (!Array.isArray(effect.model_families) || effect.model_families.length === 0) {
      throw new Error(`effect ${effect.effect_id} has no model family`);
    }
    if (!Array.isArray(effect.proof_gates) || effect.proof_gates.length < 7) {
      throw new Error(`effect ${effect.effect_id} has an incomplete proof gate set`);
    }
    if (effect.current_damage_disposition === "versioned-rule-replay-enabled"
        && effect.proof_gates.some((gate) => !["proven", "not-applicable"].includes(gate.state))) {
      throw new Error(`effect ${effect.effect_id} is enabled with an open proof gate`);
    }
    if (effect.reviewed_classification?.review_state === "non_contributing"
        && (effect.damage_relevant_model_families.length !== 0
          || effect.current_damage_disposition !== "retained-reviewed-non-damage"
          || effect.reviewed_classification.attribution_enabled !== false)) {
      throw new Error(`effect ${effect.effect_id} violates reviewed non-contributor disposition`);
    }
  }
}

function intersects(set, candidates) {
  return candidates.some((candidate) => set.has(candidate));
}

function countBy(items, selector) {
  const counts = {};
  for (const item of items) {
    const key = selector(item);
    counts[key] = (counts[key] || 0) + 1;
  }
  return Object.fromEntries(Object.entries(counts).sort(([a], [b]) => compareText(a, b)));
}

function uniqueText(values) {
  return [...new Set(values.filter((value) => typeof value === "string" && value.trim())
    .map((value) => value.trim()))].sort(compareText);
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`unexpected argument ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function readJson(filePath, label) {
  if (!existsSync(filePath)) throw new Error(`${label} not found: ${filePath}`);
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function resolvePath(value) {
  return path.resolve(repositoryRoot, value);
}

function relativePath(value) {
  return path.relative(repositoryRoot, value).replaceAll("\\", "/");
}

function compareText(left, right) {
  return String(left).localeCompare(String(right));
}

function selfTest() {
  const symbioticComponent = {
    component_key: "generic-damage",
    effect_class: "damage-modifier",
    direction: "increase",
    formula_zone_ids: ["generalDamage"],
    values: [{
      raw_text: "45%",
      source_text: "Deals damage and heals the Symbiotic Mark target for 45% of the damage dealt",
    }],
  };
  const symbiotic = {
    effect_id: 21423,
    display_name: "Symbiotic Mark",
    packet_lifecycle: { resolved_player_to_monster_windows: 6 },
    formula_endpoint: {
      formula_readiness: "description-grounded-needs-runtime-proof",
      formula_zone_ids: ["generalDamage"],
      components: [symbioticComponent],
    },
  };
  const symbioticFamilies = classifyModelFamilies(symbiotic, [symbioticComponent], null);
  assertEqual(symbioticFamilies, ["healing-support"],
    "Symbiotic Mark healing conversion must not become a damage family");

  const internalMarkerClassification = {
    review_state: "non_contributing",
    contribution_kind: "internal_marker",
    source_scope: "effect_source",
    target_scope: "party_members",
    attribution_enabled: false,
  };
  assertEqual(
    classifyModelFamilies({ display_name: "溢出CD" }, [], null, internalMarkerClassification),
    ["internal-marker"],
    "A reviewed internal marker must not be inferred as cadence damage from its design name",
  );

  assertEqual(
    classifyModelFamilies({ display_name: "Wings of Revival" }, [], null, {
      review_state: "non_contributing",
      contribution_kind: "healing_support",
      source_scope: "effect_source",
      target_scope: "effect_target",
      attribution_enabled: false,
    }),
    ["healing-support"],
    "A reviewed healing-support effect must not be inferred as outgoing damage",
  );

  assertEqual(
    classifyModelFamilies({ display_name: "Puzzle glow" }, [], null, {
      review_state: "non_contributing",
      contribution_kind: "environmental",
      source_scope: "environment",
      target_scope: "effect_target",
      attribution_enabled: false,
    }),
    ["environmental"],
    "A reviewed environmental marker must remain outside player damage attribution",
  );

  assertEqual(
    classifyModelFamilies({ display_name: "【S2套装2B】-子BUFF" }, [], null, {
      review_state: "candidate",
      contribution_kind: "state_scaling",
      source_scope: "effect_source",
      target_scope: "party_members",
      attribution_enabled: false,
    }),
    ["state-scaled-damage"],
    "A reviewed state-scaling candidate must remain damage-relevant without gaining runtime authority",
  );

  const actualDamageComponent = {
    component_key: "party-damage",
    effect_class: "damage-modifier",
    direction: "increase",
    formula_zone_ids: ["generalDamage"],
    values: [{ raw_text: "10%", source_text: "Damage dealt +10%" }],
  };
  const mixedFamilies = classifyModelFamilies({
    display_name: "Mixed support fixture",
    formula_endpoint: { formula_zone_ids: ["generalDamage"] },
    packet_lifecycle: {},
  }, [symbioticComponent, actualDamageComponent], null);
  assertEqual(mixedFamilies, ["direct-damage-multiplier", "healing-support"],
    "A separate proven damage component must survive the conversion exclusion");

  const summarized = summarizeFormulaEvidence(symbiotic.formula_endpoint);
  if (summarized.components.length !== 1
      || summarized.components[0].values[0].source_text !== symbioticComponent.values[0].source_text) {
    throw new Error("Healing-conversion classification hid canonical formula evidence");
  }

  const revokedRuntime = buildRuntimeBridges({
    harmony_grace: { effect_id: 3_003_052, runtime_transfer_enabled: false },
  });
  const revokedBridge = revokedRuntime.get(3_003_052);
  if (revokedBridge?.runtime_transfer_enabled !== false) {
    throw new Error("A revoked Harmony Grace runtime bridge regained transfer authority");
  }
  const revokedGates = buildProofGates({
    proof_queue: "external_formula_lifecycle_and_scalar_proven",
    packet_lifecycle: {
      status_events: 2,
      owner_resolved_provider_recipient_subdivision_available: true,
    },
    transfer_proof: { exact_scalar_available: true },
    formula_endpoint: { components: [] },
    packet_origins: [],
  }, revokedBridge, ["stat-conversion"], [], "external-player-to-player");
  if (revokedGates.find((entry) => entry.gate === "damage-stage-counterfactual")?.state !== "required") {
    throw new Error("A revoked runtime bridge reused stale counterfactual readiness");
  }

  const productionBridges = buildRuntimeBridges({
    game_build: "24687926",
    target_vulnerability: { runtime_transfer_effect_ids: [55_228] },
    functional_amp: { effect_id: 2_110_143, attack_magic_runtime_transfer_enabled: true },
    team_luck: {
      effect_id: 2_302_121,
      critical_damage_runtime_transfer_enabled: true,
      lucky_damage_runtime_transfer_enabled: true,
    },
    mechanical_power: { effect_id: 2_110_140, runtime_transfer_enabled: true },
    harmony_grace: { effect_id: 3_003_052, runtime_transfer_enabled: true },
    inspire: { effect_id: 31_602, runtime_transfer_enabled: true },
    stat_resonance: { effect_id: 2_207_252, runtime_transfer_enabled: true },
    fiery_battle_will: { effect_id: 2_110_065, runtime_transfer_enabled: true },
    encore: { effect_id: 55_333, runtime_transfer_enabled: true },
    highland_blood: { effect_id: 2_110_125, runtime_transfer_enabled: true },
    inspiration: {
      effect_id: 2_202_041,
      critical_chance_runtime_transfer_enabled: true,
      lucky_chance_runtime_transfer_enabled: true,
    },
    critical_cold: { effect_id: 2_204_471, runtime_transfer_enabled: true },
  });
  assertEqual(
    [...productionBridges.entries()]
      .filter(([, bridge]) => bridge.runtime_transfer_enabled)
      .map(([effectId]) => effectId)
      .sort((left, right) => left - right),
    [31_602, 55_228, 55_333, 2_110_065, 2_110_125, 2_110_140, 2_110_143,
      2_202_041, 2_204_471, 2_207_252, 2_302_121, 3_003_052],
    "Production bridge inventory must retain exactly the twelve runtime-promoted effect IDs",
  );
  console.log("rdps-external-effect-frontier self-test passed");
}

function assertEqual(actual, expected, message) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}
