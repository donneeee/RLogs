#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const inputPath = process.argv[2] ? path.resolve(process.argv[2]) : null;
const outputPath = process.argv[3] ? path.resolve(process.argv[3]) : null;
if (!inputPath || !outputPath) {
  throw new Error(
    "Usage: node tools/bpsr-source-stage-order-proof.mjs <selected-action-diagnostic.json> <output.json>",
  );
}

const inputBytes = fs.readFileSync(inputPath);
const input = JSON.parse(inputBytes);
const diagnostic = input.post_base_integer_factor_diagnostic
  ?.source_stage_order_diagnostic;
const observations = diagnostic?.observations;
if (
  Number(input.schema_version) !== 2 ||
  input.generated_by !==
    "rlogs-bpsr-target-mitigation-transform-proof:selected-ability-diagnostic" ||
  String(input.game_build) !== "24687926" ||
  JSON.stringify(input.selection?.ability_ids) !== JSON.stringify([2203521]) ||
  Number(input.selection?.hit_event_id) !== 5 ||
  Number(input.selection?.coefficient_basis_points) !== 20000 ||
  diagnostic?.authority !==
    "offline_exact_numeric_observation_input_only_not_formula_or_runtime_authority" ||
  diagnostic?.formula_authority !== false ||
  diagnostic?.runtime_authority !== false ||
  diagnostic?.provider_rdps_credit_allowed !== false ||
  JSON.stringify(diagnostic?.retained_candidate_stage_attribute_ids) !==
    JSON.stringify([11840, 11880, 11940, 11950, 12510, 12550, 12590, 12610,
      12630, 12670, 12690, 12710, 12730, 13100, 13170]) ||
  diagnostic?.missing_candidate_stage_attributes_are_omitted_not_zero !== true ||
  diagnostic?.source_and_target_attribute_and_status_state_ids_are_retained_not_expanded_or_zero_filled !== true ||
  diagnostic?.packet_calculation_and_owner_stage_context_are_retained_without_inference !== true ||
  !Array.isArray(observations) ||
  observations.length !== Number(diagnostic?.observation_count)
) {
  throw new Error("Selected-action source-stage observation input is unsafe or incomplete");
}

const STAGES = [
  "critical_damage",
  "mastery",
  "physical_amplification",
  "property_element_damage",
];
const ATTRIBUTE_BY_STAGE = {
  critical_damage: 12510,
  mastery: 11940,
  physical_amplification: 12550,
  property_element_damage: 13170,
};
const RETAINED_CANDIDATE_ATTRIBUTE_IDS = [
  11840, 11880, 11940, 11950, 12510, 12550, 12590, 12610, 12630, 12670,
  12690, 12710, 12730, 13100, 13170,
];
const OPTIONAL_PRESENCE_ATTRIBUTE_IDS = RETAINED_CANDIDATE_ATTRIBUTE_IDS
  .filter((attributeId) => !Object.values(ATTRIBUTE_BY_STAGE).includes(attributeId));

const candidates = [];
for (const order of permutations(STAGES)) {
  for (let roundingBits = 0; roundingBits < 16; roundingBits += 1) {
    const halfUpByStage = Object.fromEntries(
      STAGES.map((stage, index) => [stage, (roundingBits & (1 << index)) !== 0]),
    );
    for (let unknownFactorPosition = 0; unknownFactorPosition <= 4;
      unknownFactorPosition += 1) {
      for (const criticalAdditiveBonus of [true, false]) {
        candidates.push({
          order,
          halfUpByStage,
          unknownFactorPosition,
          criticalAdditiveBonus,
          compatible: 0,
          rejected: 0,
          unique: 0,
          compatibleByPresence: [],
          rejectedByPresence: [],
          uniqueByPresence: [],
        });
      }
    }
  }
}
if (candidates.length !== 3840) {
  throw new Error(`Expected 3840 candidates, found ${candidates.length}`);
}

const presencePatternKeys = [];
const presencePatternIndex = new Map();
const observationsByPresence = [];
for (const observation of observations) {
  const base = checkedInteger(observation.base, "base");
  const output = checkedInteger(observation.output, "output");
  checkedInteger(observation.source_attribute_state_id, "source_attribute_state_id");
  checkedInteger(observation.target_attribute_state_id, "target_attribute_state_id");
  checkedInteger(observation.source_status_state_id, "source_status_state_id");
  checkedInteger(observation.target_status_state_id, "target_status_state_id");
  if (observation.calculation_context == null || observation.owner_stage_context == null) {
    throw new Error("Selected observation lacks exact packet calculation or owner-stage context");
  }
  const raw = observation.raw_values_by_attribute_id ?? {};
  const rawByStage = Object.fromEntries(
    STAGES.map((stage) => [
      stage,
      checkedInteger(raw[String(ATTRIBUTE_BY_STAGE[stage])], `attribute ${ATTRIBUTE_BY_STAGE[stage]}`),
    ]),
  );
  const presenceKey = OPTIONAL_PRESENCE_ATTRIBUTE_IDS
    .filter((attributeId) => Object.hasOwn(raw, String(attributeId)))
    .join(",") || "<none>";
  let presenceIndex = presencePatternIndex.get(presenceKey);
  if (presenceIndex == null) {
    presenceIndex = presencePatternKeys.length;
    presencePatternKeys.push(presenceKey);
    presencePatternIndex.set(presenceKey, presenceIndex);
    observationsByPresence.push(0);
  }
  observationsByPresence[presenceIndex] += 1;
  for (const candidate of candidates) {
    const factors = {
      critical_damage: candidate.criticalAdditiveBonus
        ? checkedAdd(rawByStage.critical_damage, 10000)
        : rawByStage.critical_damage,
      mastery: checkedAdd(rawByStage.mastery, 10000),
      physical_amplification: checkedAdd(rawByStage.physical_amplification, 10000),
      property_element_damage: checkedAdd(rawByStage.property_element_damage, 10000),
    };
    let body = base;
    let valid = true;
    for (const stage of candidate.order.slice(0, candidate.unknownFactorPosition)) {
      body = fixedPointStage(body, factors[stage], candidate.halfUpByStage[stage]);
      if (body == null) {
        valid = false;
        break;
      }
    }
    let outputRange = [output, output];
    if (valid) {
      for (const stage of candidate.order
        .slice(candidate.unknownFactorPosition)
        .reverse()) {
        outputRange = fixedPointPreimageRange(
          outputRange[0],
          outputRange[1],
          factors[stage],
          candidate.halfUpByStage[stage],
        );
        if (!outputRange) {
          valid = false;
          break;
        }
      }
    }
    const interval = valid
      ? integerFactorIntervalForOutputRange(outputRange[0], outputRange[1], body)
      : null;
    if (interval) {
      candidate.compatible += 1;
      candidate.compatibleByPresence[presenceIndex] =
        (candidate.compatibleByPresence[presenceIndex] ?? 0) + 1;
      if (interval[0] === interval[1]) {
        candidate.unique += 1;
        candidate.uniqueByPresence[presenceIndex] =
          (candidate.uniqueByPresence[presenceIndex] ?? 0) + 1;
      }
    } else {
      candidate.rejected += 1;
      candidate.rejectedByPresence[presenceIndex] =
        (candidate.rejectedByPresence[presenceIndex] ?? 0) + 1;
    }
  }
}

const ranked = [...candidates].sort((left, right) =>
  left.rejected - right.rejected ||
  right.unique - left.unique ||
  left.order.join(",").localeCompare(right.order.join(",")) ||
  left.unknownFactorPosition - right.unknownFactorPosition ||
  Number(left.criticalAdditiveBonus) - Number(right.criticalAdditiveBonus) ||
  roundingKey(left).localeCompare(roundingKey(right))
);
const rejectionHistogram = new Map();
for (const candidate of candidates) {
  rejectionHistogram.set(
    candidate.rejected,
    (rejectionHistogram.get(candidate.rejected) ?? 0) + 1,
  );
}

const report = {
  schema_version: 1,
  generated_by: "tools/bpsr-source-stage-order-proof.mjs",
  game_build: "24687926",
  selection: {
    ability_id: 2203521,
    hit_event_id: 5,
    damage_attr_id: 2220352105,
    coefficient_basis_points: 20000,
  },
  policy: {
    exact_numeric_attribute_ids_and_build_are_authoritative: true,
    localized_or_enum_semantic_labels_are_evidence_only: true,
    missing_attributes_are_not_zero: true,
    remote_player_only_packets_are_required: false,
    remote_player_only_packets_are_synthesized: false,
    target_actor_allegiance_is_inferred: false,
    compatible_candidate_is_formula_authority: false,
    unresolved_evidence_is_hidden: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  },
  input: {
    path: inputPath,
    bytes: inputBytes.length,
    sha256: crypto.createHash("sha256").update(inputBytes).digest("hex"),
  },
  model_space: {
    exact_attack_coefficient_base: "floor(source_attribute_11330 * 20000 / 10000)",
    known_stage_attribute_ids: [12510, 11940, 12550, 13170],
    retained_candidate_stage_attribute_ids: RETAINED_CANDIDATE_ATTRIBUTE_IDS,
    optional_presence_partition_attribute_ids: OPTIONAL_PRESENCE_ATTRIBUTE_IDS,
    known_stage_orders: 24,
    known_stage_rounding_assignments: 16,
    critical_interpretations: 2,
    unknown_integer_factor_positions: 5,
    candidate_models: candidates.length,
    unknown_factor_model:
      "one unresolved nonnegative integer basis-point factor with floor rounding",
  },
  summary: {
    observations: observations.length,
    zero_rejection_candidates: candidates.filter((candidate) => candidate.rejected === 0)
      .length,
    minimum_rejections: ranked[0]?.rejected ?? null,
    maximum_compatible_observations: ranked[0]?.compatible ?? null,
    maximum_unique_factor_observations: Math.max(...candidates.map((candidate) => candidate.unique)),
    optional_attribute_presence_partitions: presencePatternKeys
      .map((presenceKey, index) => {
        const rankedPartition = [...candidates].sort((left, right) =>
          (left.rejectedByPresence[index] ?? 0) - (right.rejectedByPresence[index] ?? 0) ||
          (right.uniqueByPresence[index] ?? 0) - (left.uniqueByPresence[index] ?? 0) ||
          left.order.join(",").localeCompare(right.order.join(",")) ||
          left.unknownFactorPosition - right.unknownFactorPosition ||
          Number(left.criticalAdditiveBonus) - Number(right.criticalAdditiveBonus) ||
          roundingKey(left).localeCompare(roundingKey(right))
        );
        const minimumRejections = rankedPartition[0]?.rejectedByPresence[index] ?? 0;
        return {
          present_optional_attribute_ids: presenceKey === "<none>"
            ? []
            : presenceKey.split(",").map(Number),
          observations: observationsByPresence[index],
          zero_rejection_candidates: candidates.filter(
            (candidate) => (candidate.rejectedByPresence[index] ?? 0) === 0,
          ).length,
          minimum_rejections: minimumRejections,
          maximum_compatible_observations:
            rankedPartition[0]?.compatibleByPresence[index] ?? 0,
          best_candidate: candidateReportForPresence(rankedPartition[0], index),
        };
      })
      .sort((left, right) =>
        right.observations - left.observations ||
        left.present_optional_attribute_ids.join(",")
          .localeCompare(right.present_optional_attribute_ids.join(","))
      ),
    rejection_count_histogram: [...rejectionHistogram.entries()]
      .sort((left, right) => left[0] - right[0])
      .map(([rejections, candidateCount]) => ({ rejections, candidate_models: candidateCount })),
  },
  best_candidates: ranked.slice(0, 64).map(candidateReport),
  conclusion:
    "Candidate compatibility is an exhaustive arithmetic diagnostic only; no model may drive a counterfactual, runtime attribution, or UI rDPS without exact stage applicability and independent controlled replay proof.",
  formula_authority: false,
  runtime_authority: false,
  provider_rdps_credit_allowed: false,
};
report.content_sha256 = crypto
  .createHash("sha256")
  .update(JSON.stringify(report))
  .digest("hex");

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({
  output: outputPath,
  summary: {
    observations: report.summary.observations,
    candidate_models: report.model_space.candidate_models,
    zero_rejection_candidates: report.summary.zero_rejection_candidates,
    minimum_rejections: report.summary.minimum_rejections,
    maximum_compatible_observations: report.summary.maximum_compatible_observations,
    optional_attribute_presence_partition_count:
      report.summary.optional_attribute_presence_partitions.length,
  },
}, null, 2));

function* permutations(values, prefix = []) {
  if (values.length === 0) {
    yield prefix;
    return;
  }
  for (let index = 0; index < values.length; index += 1) {
    yield* permutations(
      values.filter((_, candidateIndex) => candidateIndex !== index),
      [...prefix, values[index]],
    );
  }
}

function candidateReport(candidate) {
  return {
    order: candidate.order,
    rounding_by_stage: Object.fromEntries(
      STAGES.map((stage) => [stage, candidate.halfUpByStage[stage]
        ? "nearest_half_up"
        : "floor"]),
    ),
    unknown_factor_position: candidate.unknownFactorPosition,
    critical_factor_expression: candidate.criticalAdditiveBonus
      ? "10000 + source_attribute_12510"
      : "source_attribute_12510",
    counters: {
      observations: candidate.compatible + candidate.rejected,
      compatible: candidate.compatible,
      rejected: candidate.rejected,
      unique_integer_factor: candidate.unique,
    },
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function candidateReportForPresence(candidate, presenceIndex) {
  const report = candidateReport(candidate);
  report.counters = {
    observations: (candidate.compatibleByPresence[presenceIndex] ?? 0) +
      (candidate.rejectedByPresence[presenceIndex] ?? 0),
    compatible: candidate.compatibleByPresence[presenceIndex] ?? 0,
    rejected: candidate.rejectedByPresence[presenceIndex] ?? 0,
    unique_integer_factor: candidate.uniqueByPresence[presenceIndex] ?? 0,
  };
  return report;
}

function roundingKey(candidate) {
  return STAGES.map((stage) => candidate.halfUpByStage[stage] ? "1" : "0").join("");
}

function fixedPointStage(value, factor, halfUp) {
  if (value < 0 || factor < 0) return null;
  const numerator = checkedMultiply(value, factor);
  return Math.floor((numerator + (halfUp ? 5000 : 0)) / 10000);
}

function fixedPointPreimageRange(minimumOutput, maximumOutput, factor, halfUp) {
  if (minimumOutput < 0 || maximumOutput < minimumOutput || factor <= 0) return null;
  const offset = halfUp ? 5000 : 0;
  const minimumNumerator = Math.max(
    0,
    checkedAdd(checkedMultiply(minimumOutput, 10000), -offset),
  );
  const maximumExclusiveNumerator = checkedAdd(
    checkedMultiply(checkedAdd(maximumOutput, 1), 10000),
    -offset,
  );
  const minimum = ceilDiv(minimumNumerator, factor);
  const maximum = ceilDiv(maximumExclusiveNumerator, factor) - 1;
  return minimum <= maximum ? [minimum, maximum] : null;
}

function integerFactorIntervalForOutputRange(minimumOutput, maximumOutput, base) {
  if (minimumOutput < 0 || maximumOutput < minimumOutput || base <= 0) return null;
  const minimum = ceilDiv(checkedMultiply(minimumOutput, 10000), base);
  const maximum = ceilDiv(
    checkedMultiply(checkedAdd(maximumOutput, 1), 10000),
    base,
  ) - 1;
  return minimum <= maximum ? [minimum, maximum] : null;
}

function ceilDiv(numerator, denominator) {
  if (numerator < 0 || denominator <= 0) throw new Error("Invalid ceilDiv domain");
  return Math.floor((checkedAdd(numerator, denominator - 1)) / denominator);
}

function checkedInteger(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number)) throw new Error(`Unsafe integer ${label}: ${value}`);
  return number;
}

function checkedAdd(left, right) {
  const result = left + right;
  if (!Number.isSafeInteger(result)) throw new Error("Integer addition exceeded safe range");
  return result;
}

function checkedMultiply(left, right) {
  const result = left * right;
  if (!Number.isSafeInteger(result)) throw new Error("Integer multiplication exceeded safe range");
  return result;
}
