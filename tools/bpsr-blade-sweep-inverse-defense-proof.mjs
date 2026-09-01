#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 3;
const LEGACY_SCHEMA_VERSION = 2;
const GENERATOR = "tools/bpsr-blade-sweep-inverse-defense-proof.mjs";
const SOURCE_GENERATOR = "rlogs-bpsr-status-effect-counterfactual-proof";
const SOURCE_SCHEMA_VERSION = 22;
const GAME_BUILD = "24687926";
const EFFECT_ID = 2110092;
const LOCUS = "target";
const CURVE_MODELS = [
  {
    model_id: "runtime-simple-6500",
    curve_constant: 6_500n,
    provenance:
      "exact current-build Global.AttackSimplyDefParam definition; direct combat consumer is unproven",
    combat_authority: false,
  },
  {
    model_id: "current-season-transformed-22000",
    curve_constant: 22_000n,
    provenance:
      "exact current-season FightAttrTranTable[3].DefPara and character-sheet evaluator; combat-stage binding is unproven",
    combat_authority: false,
  },
];
const BASIS_POINT_SCALE = 10_000n;
const ARMOR_PENETRATION_BASIS_POINTS = 650n;
const RETAINED_DEFENSE_NUMERATOR =
  BASIS_POINT_SCALE - ARMOR_PENETRATION_BASIS_POINTS;
const ROUNDINGS = ["floor", "ceil", "round-half-up"];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "analyze") analyze(options);
else if (command === "upgrade") upgrade(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(parsed) {
  const inputPath = path.resolve(required(parsed, "input"));
  const targetIdentityPath = path.resolve(required(parsed, "target-identity"));
  const outputPath = path.resolve(required(parsed, "output"));
  const bytes = readFileSync(inputPath);
  const targetIdentityBytes = readFileSync(targetIdentityPath);
  const source = JSON.parse(bytes.toString("utf8"));
  const targetIdentity = JSON.parse(targetIdentityBytes.toString("utf8"));
  validateSource(source);
  const effect = source.effects.find(
    (value) => Number(value?.effect_id) === EFFECT_ID && value?.locus === LOCUS,
  );
  if (!effect) throw new Error("target-locus effect 2110092 is absent");
  const exact = effect.exact_recorded_inputs;
  const examples = exact?.divergent_examples;
  if (!Array.isArray(examples) || examples.length === 0) {
    throw new Error("no exact deterministic divergent control pairs are present");
  }
  if (Number(exact.divergent_output_groups) !== examples.length) {
    throw new Error(
      "the source report truncated divergent examples; inverse proof requires every group",
    );
  }

  const pairs = examples.map((example, index) => controlledPair(example, index));
  const targetIdentityEvidence = buildTargetIdentityEvidence(
    targetIdentity,
    targetIdentityPath,
    pairs,
  );
  const targetGroups = groupBy(pairs, (pair) => pair.target_key);
  const candidateModels = CURVE_MODELS.map((curveModel) =>
    inverseCandidateModel(curveModel, targetGroups),
  );
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: GAME_BUILD,
    effect_id: EFFECT_ID,
    status: "exact-control-pairs-inversely-constrain-hidden-defense-rounding-unresolved",
    policy: {
      exact_counterfactual_pairs_required: true,
      every_divergent_group_must_be_embedded: true,
      hidden_defense_search_is_exhaustive_under_each_enumerated_candidate_model: true,
      every_exact_client_raw_physical_defense_curve_candidate_must_be_enumerated: true,
      candidate_compatibility_is_not_causal_formula_proof: true,
      observed_lucky_component_does_not_prove_upstream_pre_defense_base_formula: true,
      event_time_static_target_identity_required_for_static_stat_join: true,
      unresolved_event_time_target_identity_forbids_static_stat_join: true,
      structurally_unobservable_remote_player_packets_are_not_required: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    input: {
      path: path.basename(inputPath),
      bytes: statSync(inputPath).size,
      sha256: `sha256:${createHash("sha256").update(bytes).digest("hex")}`,
      cohort_path: path.basename(String(source.input.path)),
      cohort_bytes: Number(source.input.bytes),
      cohort_sha256: String(source.input.sha256),
      source_schema_version: Number(source.schema_version),
      source_generator: String(source.generated_by),
    },
    exact_pair_count: pairs.length,
    exact_pairs: pairs.map(publicPair),
    target_identity_evidence: targetIdentityEvidence,
    target_count: targetGroups.size,
    candidate_models: candidateModels,
    summary: {
      exact_controlled_divergent_pairs: pairs.length,
      exact_targets: targetGroups.size,
      enumerated_curve_models: candidateModels.length,
      enumerated_curve_constants: candidateModels.map((model) => model.defense_curve_constant),
      models_with_at_least_one_compatible_target: candidateModels.filter(
        (model) => model.summary.targets_with_at_least_one_compatible_variant > 0,
      ).length,
      all_curve_and_rounding_variants_remain_compatible: candidateModels.every(
        (model) => model.summary.all_rounding_variants_remain_compatible,
      ),
      minimum_hidden_defense_candidate: minimumModelCandidate(candidateModels),
      maximum_hidden_defense_candidate: maximumModelCandidate(candidateModels),
      curve_constant_selected: false,
      controlled_damage_components: [...new Set(pairs.map((pair) => pair.damage_component))],
      every_exact_pair_target_config_identity_unobserved: pairs.every(
        (pair) => !pair.target_config_identity_observed,
      ),
      every_exact_pair_target_physical_defense_unobserved: pairs.every(
        (pair) => !pair.target_physical_defense_observed,
      ),
      every_exact_pair_event_time_static_target_identity_unobserved: true,
      event_time_static_target_join_allowed: false,
      upstream_pre_defense_base_formula_proven: false,
      exact_damage_projection_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    blockers: [
      "physical-defense attribute 11350 was not observed on this target",
      "event-time actor replay over all four exact controls has active target state but no numeric monster/config ID or level, so installed static target stats cannot be joined",
      "the exact client 6500 simple and 22000 current-season transformed curves both retain compatible hidden-defense candidates",
      "floor, ceil, and round-half-up effective-defense variants remain compatible within both curve models",
      "both controls are ability 2031104 lucky_value packets, but the nonstandard AttackLucky upstream pre-defense base formula is unproven",
      "the exact controls cover one target and one lucky-damage ability family",
      "combat-stage binding, source penetration overlap, stacking arbitration, and event-level conservation remain unproven",
      "remote providers without an observed primary loadout still lack exact equipped-tier evidence",
    ],
  };
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verifyReport(JSON.parse(readFileSync(outputPath, "utf8")));
  console.log(JSON.stringify({
    status: report.status,
    exact_pair_count: report.exact_pair_count,
    candidate_models: report.candidate_models.map((model) => ({
      model_id: model.model_id,
      defense_curve_constant: model.defense_curve_constant,
      targets: model.targets.map((target) => ({
        target_key: target.target_key,
        variants: target.variants.map((variant) => ({
          rounding: variant.rounding,
          candidate_count: variant.candidate_count,
          ranges: variant.candidate_defense_ranges,
        })),
      })),
    })),
  }, null, 2));
}

function upgrade(parsed) {
  const inputPath = path.resolve(required(parsed, "input"));
  const targetIdentityPath = path.resolve(required(parsed, "target-identity"));
  const outputPath = path.resolve(required(parsed, "output"));
  const inputBytes = readFileSync(inputPath);
  const targetIdentityBytes = readFileSync(targetIdentityPath);
  const parsedInput = JSON.parse(inputBytes.toString("utf8"));
  const targetIdentity = JSON.parse(targetIdentityBytes.toString("utf8"));
  const legacy = structuredClone(parsedInput);
  if (Number(legacy.schema_version) === SCHEMA_VERSION) {
    legacy.schema_version = LEGACY_SCHEMA_VERSION;
    delete legacy.policy.event_time_static_target_identity_required_for_static_stat_join;
    delete legacy.policy.unresolved_event_time_target_identity_forbids_static_stat_join;
    delete legacy.target_identity_evidence;
    delete legacy.summary.every_exact_pair_event_time_static_target_identity_unobserved;
    delete legacy.summary.event_time_static_target_join_allowed;
    legacy.blockers = legacy.blockers.filter((value) =>
      value !==
        "event-time actor replay over all four exact controls has active target state but no numeric monster/config ID or level, so installed static target stats cannot be joined");
    legacy.content_sha256 = contentHash(legacy);
  }
  verifyReport(legacy, {
    schemaVersion: LEGACY_SCHEMA_VERSION,
    targetIdentityRequired: false,
  });
  const report = structuredClone(legacy);
  report.schema_version = SCHEMA_VERSION;
  report.policy.event_time_static_target_identity_required_for_static_stat_join = true;
  report.policy.unresolved_event_time_target_identity_forbids_static_stat_join = true;
  report.target_identity_evidence = buildTargetIdentityEvidence(
    targetIdentity,
    targetIdentityPath,
    report.exact_pairs,
  );
  report.summary.every_exact_pair_event_time_static_target_identity_unobserved = true;
  report.summary.event_time_static_target_join_allowed = false;
  const blocker =
    "event-time actor replay over all four exact controls has active target state but no numeric monster/config ID or level, so installed static target stats cannot be joined";
  if (!report.blockers.includes(blocker)) report.blockers.splice(1, 0, blocker);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verifyReport(JSON.parse(readFileSync(outputPath, "utf8")));
  console.log(JSON.stringify({
    status: report.status,
    schema_version: report.schema_version,
    exact_pair_count: report.exact_pair_count,
    matched_target_identity_actions:
      report.target_identity_evidence.summary.matched_actions,
    event_time_static_target_join_allowed:
      report.summary.event_time_static_target_join_allowed,
  }, null, 2));
}

function controlledPair(example, index) {
  if (example?.locus !== LOCUS || Number(example?.status?.effect_id) !== EFFECT_ID) {
    throw new Error(`divergent example ${index} is not target effect 2110092`);
  }
  const present = example.present_formula_context;
  const absent = example.absent_formula_context;
  if (!present || !absent ||
      present.normalized_packet_input_sha256 !== absent.normalized_packet_input_sha256 ||
      Number(present.source_attribute_state_id) !== Number(absent.source_attribute_state_id) ||
      nullableNumber(present.direct_source_attribute_state_id) !==
        nullableNumber(absent.direct_source_attribute_state_id) ||
      Number(present.target_attribute_state_id) !== Number(absent.target_attribute_state_id) ||
      stableJson(present.source_attributes) !== stableJson(absent.source_attributes) ||
      stableJson(present.direct_source_attributes) !== stableJson(absent.direct_source_attributes) ||
      stableJson(present.target_attributes) !== stableJson(absent.target_attributes) ||
      stableJson(present.source_statuses) !== stableJson(absent.source_statuses)) {
    throw new Error(`divergent example ${index} is not an exact recorded-input pair`);
  }
  const removed = removeOneStatus(present.target_statuses, example.status);
  if (stableJson(removed) !== stableJson(absent.target_statuses)) {
    throw new Error(`divergent example ${index} changes more than effect 2110092`);
  }
  const absentDamage = positiveInteger(example?.absent_outcome?.amount, "absent amount");
  const presentDamage = positiveInteger(example?.present_outcome?.amount, "present amount");
  validateLuckyOutcome(example?.absent_outcome, absentDamage, `absent outcome ${index}`);
  validateLuckyOutcome(example?.present_outcome, presentDamage, `present outcome ${index}`);
  if (presentDamage <= absentDamage) {
    throw new Error(`divergent example ${index} does not increase damage`);
  }
  const rlog = String(example.rlog);
  const sessionId = String(example.session_id);
  const runOrdinal = nonnegativeInteger(example.run_ordinal, "run ordinal");
  const targetEntityUuid = positiveInteger(example.target_entity_uuid, "target entity UUID");
  const targetAttributeStateId = nonnegativeInteger(
    absent.target_attribute_state_id,
    "target attribute state ID",
  );
  const targetAttributeIds = present.target_attributes.map((attribute) =>
    positiveInteger(attribute?.attribute_id, "target attribute ID"));
  const targetKey = [
    rlog,
    sessionId,
    runOrdinal,
    targetEntityUuid,
    targetAttributeStateId,
  ].join("|");
  return {
    index,
    target_key: targetKey,
    rlog,
    session_id: sessionId,
    run_ordinal: runOrdinal,
    source_entity_uuid: positiveInteger(example.source_entity_uuid, "source entity UUID"),
    target_entity_uuid: targetEntityUuid,
    target_attribute_state_id: targetAttributeStateId,
    ability_id: positiveInteger(example.ability_id, "ability ID"),
    normalized_packet_input_sha256: String(present.normalized_packet_input_sha256),
    provider_relationship: String(example.provider_relationship),
    status_origin_source_type_id: nullableNumber(example.status.origin_source_type_id),
    status_origin_source_config_id: nullableNumber(example.status.origin_source_config_id),
    absent_damage: absentDamage,
    present_damage: presentDamage,
    damage_component: "lucky_value",
    target_attribute_ids: targetAttributeIds,
    target_config_identity_observed: targetAttributeIds.includes(10),
    target_physical_defense_observed: targetAttributeIds.includes(11_350),
    absent_sequences: integerArray(example.absent_sequences, "absent sequences"),
    present_sequences: integerArray(example.present_sequences, "present sequences"),
    search_cutoff_exclusive_by_curve: Object.fromEntries(
      CURVE_MODELS.map((model) => [
        String(model.curve_constant),
        inverseSearchCutoff(absentDamage, presentDamage, model.curve_constant),
      ]),
    ),
  };
}

function inverseCandidateModel(curveModel, targetGroups) {
  const targets = [...targetGroups.entries()]
    .map(([targetKey, rows]) => inverseTarget(targetKey, rows, curveModel))
    .sort((left, right) => left.target_key.localeCompare(right.target_key));
  const variantSummary = ROUNDINGS.map((rounding) => ({
    rounding,
    compatible_targets: targets.filter(
      (target) => target.variants.find((value) => value.rounding === rounding).candidate_count > 0,
    ).length,
    rejected_targets: targets.filter(
      (target) => target.variants.find((value) => value.rounding === rounding).candidate_count === 0,
    ).length,
    total_candidate_defense_values: targets.reduce(
      (total, target) =>
        total + target.variants.find((value) => value.rounding === rounding).candidate_count,
      0,
    ),
  }));
  const curve = Number(curveModel.curve_constant);
  return {
    model_id: curveModel.model_id,
    provenance: curveModel.provenance,
    combat_authority: curveModel.combat_authority,
    absent_damage:
      `floor(nonnegative integer base * ${curve} / (${curve} + raw target physical defense))`,
    present_damage:
      `floor(nonnegative integer base * ${curve} / (${curve} + rounded(raw defense * 9350 / 10000)))`,
    armor_penetration_basis_points: Number(ARMOR_PENETRATION_BASIS_POINTS),
    retained_defense_basis_points: Number(RETAINED_DEFENSE_NUMERATOR),
    defense_curve_constant: curve,
    effective_defense_roundings: ROUNDINGS,
    base_is_solved_independently_for_each_exact_packet_identity: true,
    search_completeness:
      "For each pair, real present/absent damage is below (present+1)/absent. The model ratio is at least (K+D)/(K+(9350*D/10000)+1), a strictly increasing lower bound. Once that bound exceeds the observed upper ratio, no larger nonnegative defense can be compatible under floor, ceil, or round-half-up effective-defense rounding.",
    targets,
    variant_summary: variantSummary,
    summary: {
      exact_targets: targets.length,
      targets_with_at_least_one_compatible_variant: targets.filter((target) =>
        target.variants.some((variant) => variant.candidate_count > 0),
      ).length,
      all_rounding_variants_remain_compatible: variantSummary.every(
        (variant) => variant.rejected_targets === 0,
      ),
      minimum_hidden_defense_candidate: minimumCandidate(targets),
      maximum_hidden_defense_candidate: maximumCandidate(targets),
      curve_constant_selected: false,
      combat_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
}

function inverseTarget(targetKey, pairs, curveModel) {
  const curveKey = String(curveModel.curve_constant);
  const searchCutoffExclusive = Math.min(
    ...pairs.map((pair) => pair.search_cutoff_exclusive_by_curve[curveKey]),
  );
  if (!Number.isSafeInteger(searchCutoffExclusive) || searchCutoffExclusive <= 0) {
    throw new Error(`invalid inverse search cutoff for ${targetKey}`);
  }
  const variants = ROUNDINGS.map((rounding) => {
    const candidates = [];
    for (let defense = 0; defense < searchCutoffExclusive; defense += 1) {
      if (pairs.every((pair) =>
        compatiblePair(pair, defense, rounding, curveModel.curve_constant))) {
        candidates.push(defense);
      }
    }
    return {
      rounding,
      candidate_count: candidates.length,
      minimum_candidate_defense: candidates[0] ?? null,
      maximum_candidate_defense: candidates.at(-1) ?? null,
      candidate_defense_ranges: integerRanges(candidates),
      candidate_defense_values: candidates.length <= 256 ? candidates : null,
      every_exact_pair_compatible: candidates.length > 0,
    };
  });
  return {
    target_key: targetKey,
    rlog: pairs[0].rlog,
    session_id: pairs[0].session_id,
    run_ordinal: pairs[0].run_ordinal,
    target_entity_uuid: pairs[0].target_entity_uuid,
    target_attribute_state_id: pairs[0].target_attribute_state_id,
    exact_pair_count: pairs.length,
    search_minimum_defense: 0,
    search_cutoff_exclusive: searchCutoffExclusive,
    search_bound_proven_complete_for_every_larger_nonnegative_defense: true,
    variants,
    candidate_selected: false,
    exact_target_physical_defense_proven: false,
    exact_integer_rounding_proven: false,
    formula_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function compatiblePair(pair, defenseNumber, rounding, curveConstant) {
  const defense = BigInt(defenseNumber);
  const effectiveDefense = roundedEffectiveDefense(defense, rounding);
  const absentInterval = basePreimage(pair.absent_damage, defense, curveConstant);
  const presentInterval = basePreimage(pair.present_damage, effectiveDefense, curveConstant);
  return maxBigInt(absentInterval.minimum, presentInterval.minimum) <=
    minBigInt(absentInterval.maximum, presentInterval.maximum);
}

function basePreimage(damageNumber, defense, curveConstant) {
  const damage = BigInt(damageNumber);
  const denominator = curveConstant + defense;
  return {
    minimum: ceilDiv(damage * denominator, curveConstant),
    maximum: ceilDiv((damage + 1n) * denominator, curveConstant) - 1n,
  };
}

function roundedEffectiveDefense(defense, rounding) {
  const scaled = defense * RETAINED_DEFENSE_NUMERATOR;
  if (rounding === "floor") return scaled / BASIS_POINT_SCALE;
  if (rounding === "ceil") return ceilDiv(scaled, BASIS_POINT_SCALE);
  if (rounding === "round-half-up") {
    return (scaled + BASIS_POINT_SCALE / 2n) / BASIS_POINT_SCALE;
  }
  throw new Error(`unknown rounding ${rounding}`);
}

function inverseSearchCutoff(absentNumber, presentNumber, curveConstant) {
  const absent = BigInt(absentNumber);
  const presentUpper = BigInt(presentNumber) + 1n;
  // Prove that the continuous lower bound
  //   (K + D) / (K + retained*D/scale + 1)
  // exceeds the largest real ratio compatible with the two floored outputs.
  const slope = absent * BASIS_POINT_SCALE - presentUpper * RETAINED_DEFENSE_NUMERATOR;
  if (slope <= 0n) {
    throw new Error("observed ratio does not yield a finite inverse-defense search bound");
  }
  const constant =
    absent * BASIS_POINT_SCALE * curveConstant -
    presentUpper * (BASIS_POINT_SCALE * curveConstant + BASIS_POINT_SCALE);
  const cutoff = constant >= 0n ? 0n : (-constant) / slope + 1n;
  const numeric = Number(cutoff);
  if (!Number.isSafeInteger(numeric)) throw new Error("inverse search cutoff exceeds safe range");
  return numeric;
}

function removeOneStatus(statuses, selected) {
  if (!Array.isArray(statuses)) throw new Error("present target statuses are absent");
  const selectedKey = stableJson(selected);
  let removed = false;
  const values = [];
  for (const status of statuses) {
    if (!removed && stableJson(status) === selectedKey) {
      removed = true;
      continue;
    }
    values.push(status);
  }
  if (!removed) throw new Error("selected status is absent from the present target state");
  return values;
}

function publicPair(pair) {
  return {
    target_key: pair.target_key,
    rlog: pair.rlog,
    session_id: pair.session_id,
    run_ordinal: pair.run_ordinal,
    source_entity_uuid: pair.source_entity_uuid,
    target_entity_uuid: pair.target_entity_uuid,
    target_attribute_state_id: pair.target_attribute_state_id,
    ability_id: pair.ability_id,
    normalized_packet_input_sha256: pair.normalized_packet_input_sha256,
    provider_relationship: pair.provider_relationship,
    status_origin_source_type_id: pair.status_origin_source_type_id,
    status_origin_source_config_id: pair.status_origin_source_config_id,
    absent_damage: pair.absent_damage,
    present_damage: pair.present_damage,
    damage_difference: pair.present_damage - pair.absent_damage,
    damage_component: pair.damage_component,
    target_attribute_ids: pair.target_attribute_ids,
    target_config_identity_observed: pair.target_config_identity_observed,
    target_physical_defense_observed: pair.target_physical_defense_observed,
    present_to_absent_ratio_basis_points_floor:
      Math.floor(pair.present_damage * 10_000 / pair.absent_damage),
    absent_sequences: pair.absent_sequences,
    present_sequences: pair.present_sequences,
    search_cutoff_exclusive_by_curve: pair.search_cutoff_exclusive_by_curve,
    exact_recorded_input_pair: true,
  };
}

function validateLuckyOutcome(outcome, amount, name) {
  if (!outcome || outcome.normal_value != null ||
      positiveInteger(outcome.lucky_value, `${name} lucky value`) !== amount) {
    throw new Error(`${name} is not a canonical amount-equals-lucky_value packet component`);
  }
}

function buildTargetIdentityEvidence(identity, inputPath, pairs) {
  validateTargetIdentityReceipt(identity, pairs);
  const evidence = {
    artifact: {
      path: path.basename(inputPath),
      source_bytes: statSync(inputPath).size,
      canonical_json_sha256:
        `sha256:${createHash("sha256").update(JSON.stringify(identity)).digest("hex")}`,
      schema_version: Number(identity.schema_version),
      generated_by: String(identity.generated_by),
    },
    selection_source: String(identity.selection_source),
    summary: {
      requested_actions: Number(identity.summary.requested_actions),
      matched_actions: Number(identity.summary.matched_actions),
      missing_actions: Number(identity.summary.missing_actions),
      observations_with_active_actor_state:
        Number(identity.summary.observations_with_active_actor_state),
      observations_with_exact_numeric_monster_id:
        Number(identity.summary.observations_with_exact_numeric_monster_id),
      observations_with_exact_character_id:
        Number(identity.summary.observations_with_exact_character_id),
      observations_with_level: Number(identity.summary.observations_with_level),
      observations_with_identity_conflict:
        Number(identity.summary.observations_with_identity_conflict),
    },
    observations: identity.observations.map((observation) => ({
      session_id: String(observation.session_id),
      sequence: Number(observation.sequence),
      run_ordinal: Number(observation.run_ordinal),
      source_entity_uuid: Number(observation.source_entity_uuid),
      target_entity_uuid: Number(observation.target_entity_uuid),
      actor_observation_sequence: Number(observation.actor_observation_sequence),
      actor_active: observation.actor_active,
      entity_type_id: Number(observation.entity_type_id),
      actor_kind: String(observation.actor_kind),
      numeric_monster_id: observation.numeric_monster_id,
      character_id: observation.character_id,
      level: observation.level,
      identity_conflict: observation.identity_conflict,
      exact_identity_kind: String(observation.exact_identity_kind),
      static_target_stat_join_allowed: observation.static_target_stat_join_allowed,
      unresolved_reasons: observation.unresolved_reasons,
    })),
  };
  validateTargetIdentityEvidence(evidence, pairs);
  return evidence;
}

function validateTargetIdentityReceipt(identity, pairs) {
  if (Number(identity?.schema_version) !== 2 ||
      identity?.generated_by !== "rlogs-bpsr-selected-action-target-identity-proof" ||
      String(identity?.game_build) !== GAME_BUILD ||
      identity?.selection_source !== "blade-sweep-inverse-defense-proof.v1.json" ||
      identity?.policy?.static_target_stats_substituted !== false ||
      identity?.policy?.runtime_authority !== false ||
      identity?.policy?.provider_rdps_credit_allowed !== false ||
      !Array.isArray(identity?.missing_action_keys) ||
      identity.missing_action_keys.length !== 0 ||
      !Array.isArray(identity?.observations)) {
    throw new Error("target-identity receipt is not the reviewed fail-closed report");
  }
  validateTargetIdentityRows(identity.summary, identity.observations, pairs);
}

function validateTargetIdentityEvidence(evidence, pairs) {
  if (evidence?.artifact?.path !== "blade-sweep-target-identity-proof.v1.json" ||
      !Number.isSafeInteger(evidence?.artifact?.source_bytes) ||
      evidence.artifact.source_bytes <= 0 ||
      !/^sha256:[0-9a-f]{64}$/.test(
        String(evidence?.artifact?.canonical_json_sha256)) ||
      Number(evidence?.artifact?.schema_version) !== 2 ||
      evidence?.artifact?.generated_by !==
        "rlogs-bpsr-selected-action-target-identity-proof" ||
      evidence?.selection_source !== "blade-sweep-inverse-defense-proof.v1.json" ||
      !Array.isArray(evidence?.observations)) {
    throw new Error("embedded target-identity evidence has invalid provenance");
  }
  validateTargetIdentityRows(evidence.summary, evidence.observations, pairs);
}

function validateTargetIdentityRows(summary, observations, pairs) {
  const expected = pairs.flatMap((pair) =>
    [...integerArray(pair.absent_sequences, "absent sequences"),
      ...integerArray(pair.present_sequences, "present sequences")].map((sequence) => ({
      session_id: String(pair.session_id),
      sequence,
      run_ordinal: nonnegativeInteger(pair.run_ordinal, "run ordinal"),
      source_entity_uuid: positiveInteger(pair.source_entity_uuid, "source entity UUID"),
      target_entity_uuid: positiveInteger(pair.target_entity_uuid, "target entity UUID"),
    })),
  ).sort(compareActionIdentity);
  const actual = observations.map((observation, index) => {
    const row = {
      session_id: String(observation?.session_id),
      sequence: positiveInteger(observation?.sequence, `identity sequence ${index}`),
      run_ordinal: nonnegativeInteger(observation?.run_ordinal, `identity run ${index}`),
      source_entity_uuid:
        positiveInteger(observation?.source_entity_uuid, `identity source ${index}`),
      target_entity_uuid:
        positiveInteger(observation?.target_entity_uuid, `identity target ${index}`),
    };
    if (!Number.isSafeInteger(Number(observation?.actor_observation_sequence)) ||
        Number(observation.actor_observation_sequence) >= row.sequence ||
        observation?.actor_active !== true ||
        Number(observation?.entity_type_id) !== 0 ||
        observation?.actor_kind !== "unknown" ||
        observation?.numeric_monster_id != null ||
        observation?.character_id != null ||
        observation?.level != null ||
        observation?.identity_conflict !== false ||
        observation?.exact_identity_kind !== "unresolved" ||
        observation?.static_target_stat_join_allowed !== false ||
        stableJson(observation?.unresolved_reasons) !==
          stableJson(["exact-static-target-identity-absent"])) {
      throw new Error(`target-identity observation ${index} is not fail-closed`);
    }
    return row;
  }).sort(compareActionIdentity);
  if (stableJson(actual) !== stableJson(expected)) {
    throw new Error("target-identity observations do not exactly cover every control action");
  }
  const expectedCount = expected.length;
  if (Number(summary?.requested_actions) !== expectedCount ||
      Number(summary?.matched_actions) !== expectedCount ||
      Number(summary?.missing_actions) !== 0 ||
      Number(summary?.observations_with_active_actor_state) !== expectedCount ||
      Number(summary?.observations_with_exact_numeric_monster_id) !== 0 ||
      Number(summary?.observations_with_exact_character_id) !== 0 ||
      Number(summary?.observations_with_level) !== 0 ||
      Number(summary?.observations_with_identity_conflict) !== 0) {
    throw new Error("target-identity summary does not match the exact control coverage");
  }
}

function compareActionIdentity(left, right) {
  return left.session_id.localeCompare(right.session_id) ||
    left.sequence - right.sequence || left.run_ordinal - right.run_ordinal;
}

function validateSource(source) {
  if (Number(source?.schema_version) !== SOURCE_SCHEMA_VERSION ||
      source?.generated_by !== SOURCE_GENERATOR ||
      String(source?.game_build) !== GAME_BUILD ||
      source?.policy?.formula_authority !== false ||
      source?.policy?.runtime_authority !== false ||
      source?.policy?.candidate_projection_authority !== false ||
      !Array.isArray(source?.effects) ||
      typeof source?.input?.sha256 !== "string") {
    throw new Error("counterfactual source is not the reviewed schema-22 fail-closed report");
  }
}

function verifyCommand(parsed) {
  const inputPath = path.resolve(required(parsed, "input"));
  const report = JSON.parse(readFileSync(inputPath, "utf8"));
  verifyReport(report);
  verifyTargetIdentityArtifact(report, inputPath);
  console.log("Blade Sweep inverse-defense proof verified");
}

function verifyTargetIdentityArtifact(report, reportPath) {
  const expected = report.target_identity_evidence.artifact;
  const artifactPath = path.join(path.dirname(reportPath), expected.path);
  const bytes = readFileSync(artifactPath);
  const identity = JSON.parse(bytes.toString("utf8"));
  const semanticHash =
    `sha256:${createHash("sha256").update(JSON.stringify(identity)).digest("hex")}`;
  if (semanticHash !== expected.canonical_json_sha256) {
    throw new Error("target-identity artifact JSON does not match the embedded receipt");
  }
  validateTargetIdentityReceipt(identity, report.exact_pairs);
}

function verifyReport(report, {
  schemaVersion = SCHEMA_VERSION,
  targetIdentityRequired = true,
} = {}) {
  if (Number(report?.schema_version) !== schemaVersion ||
      report?.generated_by !== GENERATOR ||
      String(report?.game_build) !== GAME_BUILD ||
      Number(report?.effect_id) !== EFFECT_ID ||
      report?.status !== "exact-control-pairs-inversely-constrain-hidden-defense-rounding-unresolved" ||
      report?.content_sha256 !== contentHash(report) ||
      report?.policy?.hidden_defense_search_is_exhaustive_under_each_enumerated_candidate_model !== true ||
      report?.policy
        ?.every_exact_client_raw_physical_defense_curve_candidate_must_be_enumerated !== true ||
      report?.policy
        ?.observed_lucky_component_does_not_prove_upstream_pre_defense_base_formula !== true ||
      report?.policy?.formula_authority !== false ||
      report?.summary?.exact_controlled_divergent_pairs < 1 ||
      report?.summary?.models_with_at_least_one_compatible_target < 1 ||
      report?.summary?.curve_constant_selected !== false ||
      stableJson(report?.summary?.controlled_damage_components) !==
        stableJson(["lucky_value"]) ||
      report?.summary?.every_exact_pair_target_config_identity_unobserved !== true ||
      report?.summary?.every_exact_pair_target_physical_defense_unobserved !== true ||
      report?.summary?.upstream_pre_defense_base_formula_proven !== false ||
      report?.summary?.exact_damage_projection_proven !== false ||
      report?.summary?.exact_operation_order_proven !== false ||
      report?.summary?.exact_integer_rounding_proven !== false ||
      report?.summary?.packet_conservation_proven !== false ||
      report?.summary?.formula_authority !== false ||
      report?.summary?.runtime_authority !== false ||
      report?.summary?.ui_display_authority !== false ||
      report?.summary?.provider_rdps_credit_allowed !== false ||
      !Array.isArray(report?.candidate_models) ||
      report.candidate_models.length !== CURVE_MODELS.length ||
      report.candidate_models.some((model, index) =>
        model?.model_id !== CURVE_MODELS[index].model_id ||
        Number(model?.defense_curve_constant) !== Number(CURVE_MODELS[index].curve_constant) ||
        model?.combat_authority !== false ||
        model?.summary?.curve_constant_selected !== false ||
        model?.summary?.combat_authority !== false ||
        model?.summary?.provider_rdps_credit_allowed !== false ||
        !Array.isArray(model?.targets) || model.targets.length < 1 ||
        model.targets.some((target) =>
          target?.search_bound_proven_complete_for_every_larger_nonnegative_defense !== true ||
          target?.candidate_selected !== false ||
          target?.exact_target_physical_defense_proven !== false ||
          target?.exact_integer_rounding_proven !== false ||
          target?.formula_authority !== false ||
          target?.provider_rdps_credit_allowed !== false ||
          !Array.isArray(target?.variants) || target.variants.length !== ROUNDINGS.length ||
          target.variants.some((variant, variantIndex) =>
            variant?.rounding !== ROUNDINGS[variantIndex] ||
            !Number.isSafeInteger(variant?.candidate_count) ||
            variant.candidate_count < 0 ||
            variant?.every_exact_pair_compatible !== (variant.candidate_count > 0),
          )
        ),
      )) {
    throw new Error("inverse-defense report failed its fail-closed verification contract");
  }
  if (targetIdentityRequired &&
      (schemaVersion !== SCHEMA_VERSION ||
       report?.policy?.event_time_static_target_identity_required_for_static_stat_join !== true ||
       report?.policy?.unresolved_event_time_target_identity_forbids_static_stat_join !== true ||
       report?.summary?.every_exact_pair_event_time_static_target_identity_unobserved !== true ||
       report?.summary?.event_time_static_target_join_allowed !== false ||
       !Array.isArray(report?.blockers) ||
       !report.blockers.includes(
         "event-time actor replay over all four exact controls has active target state but no numeric monster/config ID or level, so installed static target stats cannot be joined"
       ))) {
    throw new Error("inverse-defense report lacks the event-time target-identity gate");
  }
  if (!Array.isArray(report.exact_pairs) ||
      report.exact_pairs.length !== report.exact_pair_count) {
    throw new Error("inverse-defense report does not retain every exact pair receipt");
  }
  const pairs = report.exact_pairs.map((pair, index) => {
    const absentDamage = positiveInteger(pair?.absent_damage, "verified absent damage");
    const presentDamage = positiveInteger(pair?.present_damage, "verified present damage");
    const searchCutoffExclusiveByCurve = Object.fromEntries(
      CURVE_MODELS.map((model) => [
        String(model.curve_constant),
        inverseSearchCutoff(absentDamage, presentDamage, model.curve_constant),
      ]),
    );
    if (presentDamage <= absentDamage ||
        Number(pair?.damage_difference) !== presentDamage - absentDamage ||
        Number(pair?.present_to_absent_ratio_basis_points_floor) !==
          Math.floor(presentDamage * 10_000 / absentDamage) ||
        stableJson(pair?.search_cutoff_exclusive_by_curve) !==
          stableJson(searchCutoffExclusiveByCurve) ||
        pair?.damage_component !== "lucky_value" ||
        stableJson(pair?.target_attribute_ids) !== stableJson([11_310]) ||
        pair?.target_config_identity_observed !== false ||
        pair?.target_physical_defense_observed !== false ||
        pair?.exact_recorded_input_pair !== true) {
      throw new Error(`inverse-defense exact pair ${index} failed recomputation`);
    }
    return {
      ...pair,
      absent_damage: absentDamage,
      present_damage: presentDamage,
      search_cutoff_exclusive_by_curve: searchCutoffExclusiveByCurve,
    };
  });
  if (targetIdentityRequired) {
    validateTargetIdentityEvidence(report.target_identity_evidence, pairs);
  }
  const targetGroups = groupBy(pairs, (pair) => pair.target_key);
  const recomputedModels = CURVE_MODELS.map((model) =>
    inverseCandidateModel(model, targetGroups),
  );
  if (stableJson(recomputedModels) !== stableJson(report.candidate_models)) {
    throw new Error("inverse-defense models, targets, or candidate sets failed exact recomputation");
  }
  if (Number(report.target_count) !== targetGroups.size ||
      Number(report.summary.exact_targets) !== targetGroups.size ||
      Number(report.summary.enumerated_curve_models) !== recomputedModels.length ||
      stableJson(report.summary.enumerated_curve_constants) !==
        stableJson(recomputedModels.map((model) => model.defense_curve_constant)) ||
      Number(report.summary.models_with_at_least_one_compatible_target) !==
        recomputedModels.filter(
          (model) => model.summary.targets_with_at_least_one_compatible_variant > 0,
        ).length ||
      report.summary.all_curve_and_rounding_variants_remain_compatible !==
        recomputedModels.every((model) =>
          model.summary.all_rounding_variants_remain_compatible) ||
      Number(report.summary.minimum_hidden_defense_candidate) !==
        minimumModelCandidate(recomputedModels) ||
      Number(report.summary.maximum_hidden_defense_candidate) !==
        maximumModelCandidate(recomputedModels)) {
    throw new Error("inverse-defense summary failed exact recomputation");
  }
}

function selfTest() {
  const pairs = [
    { absent_damage: 78_266, present_damage: 80_211 },
    { absent_damage: 96_580, present_damage: 98_980 },
  ].map((pair) => ({
    ...pair,
    search_cutoff_exclusive_by_curve: Object.fromEntries(
      CURVE_MODELS.map((model) => [
        String(model.curve_constant),
        inverseSearchCutoff(pair.absent_damage, pair.present_damage, model.curve_constant),
      ]),
    ),
  }));
  const rows = pairs.map((pair) => ({
    ...pair,
    rlog: "fixture.rlog",
    session_id: "fixture",
    run_ordinal: 0,
    target_entity_uuid: 1,
    target_attribute_state_id: 2,
  }));
  const expected = {
    "runtime-simple-6500": {
      floor: [3_850, 3_851, 3_852, 3_853, 3_854],
      ceil: [3_891, 3_892],
      "round-half-up": [3_854],
    },
    "current-season-transformed-22000": {
      floor: [13_062, 13_064, 13_066, 13_087, 13_089, 13_091, 13_092],
      ceil: [13_093, 13_094, 13_095, 13_096, 13_097, 13_098, 13_099, 13_100,
        13_101, 13_102, 13_103, 13_104, 13_105, 13_107],
      "round-half-up": [13_087, 13_089, 13_091, 13_092, 13_093, 13_094,
        13_095, 13_096, 13_097, 13_098, 13_099, 13_100],
    },
  };
  for (const model of CURVE_MODELS) {
    const target = inverseTarget("fixture", rows, model);
    for (const variant of target.variants) {
      if (stableJson(variant.candidate_defense_values) !==
          stableJson(expected[model.model_id][variant.rounding])) {
        throw new Error(`inverse fixture changed for ${model.model_id}/${variant.rounding}`);
      }
    }
  }
  console.log("Blade Sweep inverse-defense proof self-test passed");
}

function integerRanges(values) {
  if (values.length === 0) return [];
  const ranges = [];
  let start = values[0];
  let previous = start;
  for (const value of values.slice(1)) {
    if (value !== previous + 1) {
      ranges.push({ minimum: start, maximum: previous });
      start = value;
    }
    previous = value;
  }
  ranges.push({ minimum: start, maximum: previous });
  return ranges;
}

function minimumCandidate(targets) {
  const values = targets.flatMap((target) =>
    target.variants.flatMap((variant) =>
      variant.minimum_candidate_defense == null ? [] : [variant.minimum_candidate_defense]),
  );
  return values.length ? Math.min(...values) : null;
}

function maximumCandidate(targets) {
  const values = targets.flatMap((target) =>
    target.variants.flatMap((variant) =>
      variant.maximum_candidate_defense == null ? [] : [variant.maximum_candidate_defense]),
  );
  return values.length ? Math.max(...values) : null;
}

function minimumModelCandidate(models) {
  const values = models.flatMap((model) => {
    const value = model?.summary?.minimum_hidden_defense_candidate;
    return value == null ? [] : [Number(value)];
  });
  return values.length ? Math.min(...values) : null;
}

function maximumModelCandidate(models) {
  const values = models.flatMap((model) => {
    const value = model?.summary?.maximum_hidden_defense_candidate;
    return value == null ? [] : [Number(value)];
  });
  return values.length ? Math.max(...values) : null;
}

function groupBy(values, key) {
  const groups = new Map();
  for (const value of values) {
    const groupKey = key(value);
    const rows = groups.get(groupKey) ?? [];
    rows.push(value);
    groups.set(groupKey, rows);
  }
  return groups;
}

function ceilDiv(numerator, denominator) {
  return (numerator + denominator - 1n) / denominator;
}

function minBigInt(left, right) {
  return left < right ? left : right;
}

function maxBigInt(left, right) {
  return left > right ? left : right;
}

function stableJson(value) {
  return JSON.stringify(value);
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return `sha256:${createHash("sha256").update(JSON.stringify(clone)).digest("hex")}`;
}

function positiveInteger(value, name) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number <= 0) throw new Error(`${name} must be positive`);
  return number;
}

function nonnegativeInteger(value, name) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 0) throw new Error(`${name} must be nonnegative`);
  return number;
}

function nullableNumber(value) {
  return value == null ? null : Number(value);
}

function integerArray(value, name) {
  if (!Array.isArray(value)) throw new Error(`${name} must be an array`);
  return value.map((entry) => nonnegativeInteger(entry, name));
}

function parseArgs(values) {
  const parsed = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    if (!flag?.startsWith("--") || value == null) usage(1);
    parsed.set(flag.slice(2), value);
  }
  return parsed;
}

function required(parsed, name) {
  const value = parsed.get(name);
  if (!value) throw new Error(`--${name} is required`);
  return value;
}

function usage(exitCode) {
  console.log(
    "Usage:\n" +
    "  node tools/bpsr-blade-sweep-inverse-defense-proof.mjs analyze --input <counterfactual.json> --target-identity <identity.json> --output <proof.json>\n" +
    "  node tools/bpsr-blade-sweep-inverse-defense-proof.mjs upgrade --input <schema-2-proof.json> --target-identity <identity.json> --output <schema-3-proof.json>\n" +
    "  node tools/bpsr-blade-sweep-inverse-defense-proof.mjs verify --input <proof.json>\n" +
    "  node tools/bpsr-blade-sweep-inverse-defense-proof.mjs self-test",
  );
  process.exit(exitCode);
}
