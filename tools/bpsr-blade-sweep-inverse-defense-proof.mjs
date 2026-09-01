#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-blade-sweep-inverse-defense-proof.mjs";
const SOURCE_GENERATOR = "rlogs-bpsr-status-effect-counterfactual-proof";
const SOURCE_SCHEMA_VERSION = 22;
const GAME_BUILD = "24687926";
const EFFECT_ID = 2110092;
const LOCUS = "target";
const CURVE_CONSTANT = 22_000n;
const BASIS_POINT_SCALE = 10_000n;
const ARMOR_PENETRATION_BASIS_POINTS = 650n;
const RETAINED_DEFENSE_NUMERATOR =
  BASIS_POINT_SCALE - ARMOR_PENETRATION_BASIS_POINTS;
const ROUNDINGS = ["floor", "ceil", "round-half-up"];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "analyze") analyze(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(parsed) {
  const inputPath = path.resolve(required(parsed, "input"));
  const outputPath = path.resolve(required(parsed, "output"));
  const bytes = readFileSync(inputPath);
  const source = JSON.parse(bytes.toString("utf8"));
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
  const targetGroups = groupBy(pairs, (pair) => pair.target_key);
  const targets = [...targetGroups.entries()]
    .map(([targetKey, rows]) => inverseTarget(targetKey, rows))
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
      candidate_compatibility_is_not_causal_formula_proof: true,
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
    candidate_model: {
      absent_damage:
        "floor(nonnegative integer base * 22000 / (22000 + raw target physical defense))",
      present_damage:
        "floor(nonnegative integer base * 22000 / (22000 + rounded(raw defense * 9350 / 10000)))",
      armor_penetration_basis_points: Number(ARMOR_PENETRATION_BASIS_POINTS),
      retained_defense_basis_points: Number(RETAINED_DEFENSE_NUMERATOR),
      defense_curve_constant: Number(CURVE_CONSTANT),
      effective_defense_roundings: ROUNDINGS,
      base_is_solved_independently_for_each_exact_packet_identity: true,
      search_completeness:
        "For each pair, real present/absent damage is below (present+1)/absent. The model ratio is at least (K+D)/(K+(9350*D/10000)+1), a strictly increasing lower bound. Once that bound exceeds the observed upper ratio, no larger nonnegative defense can be compatible under floor, ceil, or round-half-up effective-defense rounding.",
    },
    exact_pair_count: pairs.length,
    target_count: targets.length,
    exact_pairs: pairs.map(publicPair),
    targets,
    variant_summary: variantSummary,
    summary: {
      exact_controlled_divergent_pairs: pairs.length,
      exact_targets: targets.length,
      targets_with_at_least_one_compatible_model: targets.filter((target) =>
        target.variants.some((variant) => variant.candidate_count > 0),
      ).length,
      all_rounding_variants_remain_compatible: variantSummary.every(
        (variant) => variant.rejected_targets === 0,
      ),
      minimum_hidden_defense_candidate: minimumCandidate(targets),
      maximum_hidden_defense_candidate: maximumCandidate(targets),
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
      "floor, ceil, and round-half-up effective-defense variants retain compatible hidden-defense candidates",
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
    targets: report.targets.map((target) => ({
      target_key: target.target_key,
      variants: target.variants.map((variant) => ({
        rounding: variant.rounding,
        candidate_count: variant.candidate_count,
        ranges: variant.candidate_defense_ranges,
      })),
    })),
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
    absent_sequences: integerArray(example.absent_sequences, "absent sequences"),
    present_sequences: integerArray(example.present_sequences, "present sequences"),
    search_cutoff_exclusive: inverseSearchCutoff(absentDamage, presentDamage),
  };
}

function inverseTarget(targetKey, pairs) {
  const searchCutoffExclusive = Math.min(...pairs.map((pair) => pair.search_cutoff_exclusive));
  if (!Number.isSafeInteger(searchCutoffExclusive) || searchCutoffExclusive <= 0) {
    throw new Error(`invalid inverse search cutoff for ${targetKey}`);
  }
  const variants = ROUNDINGS.map((rounding) => {
    const candidates = [];
    for (let defense = 0; defense < searchCutoffExclusive; defense += 1) {
      if (pairs.every((pair) => compatiblePair(pair, defense, rounding))) {
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

function compatiblePair(pair, defenseNumber, rounding) {
  const defense = BigInt(defenseNumber);
  const effectiveDefense = roundedEffectiveDefense(defense, rounding);
  const absentInterval = basePreimage(pair.absent_damage, defense);
  const presentInterval = basePreimage(pair.present_damage, effectiveDefense);
  return maxBigInt(absentInterval.minimum, presentInterval.minimum) <=
    minBigInt(absentInterval.maximum, presentInterval.maximum);
}

function basePreimage(damageNumber, defense) {
  const damage = BigInt(damageNumber);
  const denominator = CURVE_CONSTANT + defense;
  return {
    minimum: ceilDiv(damage * denominator, CURVE_CONSTANT),
    maximum: ceilDiv((damage + 1n) * denominator, CURVE_CONSTANT) - 1n,
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

function inverseSearchCutoff(absentNumber, presentNumber) {
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
    absent * BASIS_POINT_SCALE * CURVE_CONSTANT -
    presentUpper * (BASIS_POINT_SCALE * CURVE_CONSTANT + BASIS_POINT_SCALE);
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
    present_to_absent_ratio_basis_points_floor:
      Math.floor(pair.present_damage * 10_000 / pair.absent_damage),
    absent_sequences: pair.absent_sequences,
    present_sequences: pair.present_sequences,
    search_cutoff_exclusive: pair.search_cutoff_exclusive,
    exact_recorded_input_pair: true,
  };
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
  verifyReport(JSON.parse(readFileSync(inputPath, "utf8")));
  console.log("Blade Sweep inverse-defense proof verified");
}

function verifyReport(report) {
  if (Number(report?.schema_version) !== SCHEMA_VERSION ||
      report?.generated_by !== GENERATOR ||
      String(report?.game_build) !== GAME_BUILD ||
      Number(report?.effect_id) !== EFFECT_ID ||
      report?.status !== "exact-control-pairs-inversely-constrain-hidden-defense-rounding-unresolved" ||
      report?.content_sha256 !== contentHash(report) ||
      report?.policy?.hidden_defense_search_is_exhaustive_under_each_enumerated_candidate_model !== true ||
      report?.policy?.formula_authority !== false ||
      report?.summary?.exact_controlled_divergent_pairs < 1 ||
      report?.summary?.targets_with_at_least_one_compatible_model < 1 ||
      report?.summary?.exact_damage_projection_proven !== false ||
      report?.summary?.exact_operation_order_proven !== false ||
      report?.summary?.exact_integer_rounding_proven !== false ||
      report?.summary?.packet_conservation_proven !== false ||
      report?.summary?.formula_authority !== false ||
      report?.summary?.runtime_authority !== false ||
      report?.summary?.ui_display_authority !== false ||
      report?.summary?.provider_rdps_credit_allowed !== false ||
      !Array.isArray(report?.targets) || report.targets.length < 1 ||
      report.targets.some((target) =>
        target?.search_bound_proven_complete_for_every_larger_nonnegative_defense !== true ||
        target?.candidate_selected !== false ||
        target?.exact_target_physical_defense_proven !== false ||
        target?.exact_integer_rounding_proven !== false ||
        target?.formula_authority !== false ||
        target?.provider_rdps_credit_allowed !== false ||
        !Array.isArray(target?.variants) || target.variants.length !== ROUNDINGS.length ||
        target.variants.some((variant, index) =>
          variant?.rounding !== ROUNDINGS[index] ||
          !Number.isSafeInteger(variant?.candidate_count) ||
          variant.candidate_count < 0 ||
          variant?.every_exact_pair_compatible !== (variant.candidate_count > 0),
        )
      )) {
    throw new Error("inverse-defense report failed its fail-closed verification contract");
  }
  if (!Array.isArray(report.exact_pairs) ||
      report.exact_pairs.length !== report.exact_pair_count) {
    throw new Error("inverse-defense report does not retain every exact pair receipt");
  }
  const pairs = report.exact_pairs.map((pair, index) => {
    const absentDamage = positiveInteger(pair?.absent_damage, "verified absent damage");
    const presentDamage = positiveInteger(pair?.present_damage, "verified present damage");
    const searchCutoffExclusive = inverseSearchCutoff(absentDamage, presentDamage);
    if (presentDamage <= absentDamage ||
        Number(pair?.damage_difference) !== presentDamage - absentDamage ||
        Number(pair?.present_to_absent_ratio_basis_points_floor) !==
          Math.floor(presentDamage * 10_000 / absentDamage) ||
        Number(pair?.search_cutoff_exclusive) !== searchCutoffExclusive ||
        pair?.exact_recorded_input_pair !== true) {
      throw new Error(`inverse-defense exact pair ${index} failed recomputation`);
    }
    return {
      ...pair,
      absent_damage: absentDamage,
      present_damage: presentDamage,
      search_cutoff_exclusive: searchCutoffExclusive,
    };
  });
  const recomputedTargets = [...groupBy(pairs, (pair) => pair.target_key).entries()]
    .map(([targetKey, rows]) => inverseTarget(targetKey, rows))
    .sort((left, right) => left.target_key.localeCompare(right.target_key));
  if (stableJson(recomputedTargets) !== stableJson(report.targets)) {
    throw new Error("inverse-defense targets or candidate sets failed exact recomputation");
  }
  const recomputedVariantSummary = ROUNDINGS.map((rounding) => ({
    rounding,
    compatible_targets: recomputedTargets.filter(
      (target) => target.variants.find((value) => value.rounding === rounding).candidate_count > 0,
    ).length,
    rejected_targets: recomputedTargets.filter(
      (target) => target.variants.find((value) => value.rounding === rounding).candidate_count === 0,
    ).length,
    total_candidate_defense_values: recomputedTargets.reduce(
      (total, target) =>
        total + target.variants.find((value) => value.rounding === rounding).candidate_count,
      0,
    ),
  }));
  if (stableJson(recomputedVariantSummary) !== stableJson(report.variant_summary) ||
      Number(report.summary.minimum_hidden_defense_candidate) !==
        minimumCandidate(recomputedTargets) ||
      Number(report.summary.maximum_hidden_defense_candidate) !==
        maximumCandidate(recomputedTargets) ||
      Number(report.summary.exact_targets) !== recomputedTargets.length ||
      Number(report.summary.targets_with_at_least_one_compatible_model) !==
        recomputedTargets.filter((target) =>
          target.variants.some((variant) => variant.candidate_count > 0),
        ).length ||
      report.summary.all_rounding_variants_remain_compatible !==
        recomputedVariantSummary.every((variant) => variant.rejected_targets === 0)) {
    throw new Error("inverse-defense summary failed exact recomputation");
  }
}

function selfTest() {
  const pairs = [
    { absent_damage: 78_266, present_damage: 80_211 },
    { absent_damage: 96_580, present_damage: 98_980 },
  ].map((pair) => ({
    ...pair,
    search_cutoff_exclusive: inverseSearchCutoff(pair.absent_damage, pair.present_damage),
  }));
  const target = inverseTarget("fixture", pairs.map((pair) => ({
    ...pair,
    rlog: "fixture.rlog",
    session_id: "fixture",
    run_ordinal: 0,
    target_entity_uuid: 1,
    target_attribute_state_id: 2,
  })));
  const expected = {
    floor: [13_062, 13_064, 13_066, 13_087, 13_089, 13_091, 13_092],
    ceil: [13_093, 13_094, 13_095, 13_096, 13_097, 13_098, 13_099, 13_100,
      13_101, 13_102, 13_103, 13_104, 13_105, 13_107],
    "round-half-up": [13_087, 13_089, 13_091, 13_092, 13_093, 13_094,
      13_095, 13_096, 13_097, 13_098, 13_099, 13_100],
  };
  for (const variant of target.variants) {
    if (stableJson(variant.candidate_defense_values) !== stableJson(expected[variant.rounding])) {
      throw new Error(`inverse fixture changed for ${variant.rounding}`);
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
    "  node tools/bpsr-blade-sweep-inverse-defense-proof.mjs analyze --input <counterfactual.json> --output <proof.json>\n" +
    "  node tools/bpsr-blade-sweep-inverse-defense-proof.mjs verify --input <proof.json>\n" +
    "  node tools/bpsr-blade-sweep-inverse-defense-proof.mjs self-test",
  );
  process.exit(exitCode);
}
