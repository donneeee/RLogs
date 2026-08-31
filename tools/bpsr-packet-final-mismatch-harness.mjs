#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const SCHEMA_VERSION = 1;
const SEED_SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-packet-final-mismatch-harness.mjs";
const BASIS_POINT_SCALE = 10_000n;
const MAX_INPUT_BYTES = 32 * 1024 * 1024;
const MULTIPLIER_STAGES = [
  "versatility",
  "element",
  "generic",
  "dream",
  "boost",
  "other",
  "final",
];
const ROUNDING_STAGES = [
  "attack_after_resistance",
  "skill_base",
  ...MULTIPLIER_STAGES,
  "critical",
  "packet_final",
];
const ROUNDING_MODES = new Set(["defer", "floor", "ceil", "positive_half_up"]);

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(parsed) {
  const seedPath = resolved(required(parsed, "seed"));
  const output = resolved(required(parsed, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);
  const report = buildReport(seedPath);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote ${output}`);
}

function buildReport(seedPath) {
  const seed = readBoundedJson(seedPath, "mismatch seed");
  assert(seed?.schema_version === SEED_SCHEMA_VERSION, "Mismatch seed schema_version must be 1");
  assert(typeof seed.game_build === "string" && seed.game_build.length > 0, "Seed game_build is required");

  const seedDirectory = path.dirname(seedPath);
  const contextPath = sourcePath(seedDirectory, seed.sample_context?.path, "sample context");
  const damageStagePath = sourcePath(seedDirectory, seed.damage_stage?.path, "damage-stage catalog");
  const ledgerPath = sourcePath(seedDirectory, seed.candidate_ledger?.path, "candidate ledger");
  const context = readBoundedJson(contextPath, "sample context");
  const damageStage = readBoundedJson(damageStagePath, "damage-stage catalog");
  const ledger = readBoundedJson(ledgerPath, "candidate ledger");

  assert(context?.schema_version === 2 &&
    context?.generated_by === "tools/bpsr-selected-pair-formula-context.mjs",
  "Sample context must be the bounded selected-pair schema 2 artifact");
  requireBuild(context, seed.game_build, "sample context");
  requireBuild(damageStage, seed.game_build, "damage-stage catalog");
  requireBuild(ledger, seed.game_build, "candidate ledger");
  assert(Array.isArray(damageStage.rules), "Damage-stage catalog rules are required");

  const selection = seed.selection ?? {};
  const sequence = integerNumber(selection.sequence, "selection.sequence");
  const abilityId = integerNumber(selection.ability_id, "selection.ability_id");
  const hitEventId = integerNumber(selection.hit_event_id, "selection.hit_event_id");
  const damageAttrId = integerNumber(selection.damage_attr_id, "selection.damage_attr_id");
  const selectedPacketFinal = integerNumber(selection.packet_final_integer,
    "selection.packet_final_integer");
  const sample = only((context.selected_samples ?? []).filter((entry) =>
    integerNumber(entry.sequence, "sample.sequence") === sequence), "selected packet sample");
  assert(integerNumber(sample.ability_id, "sample.ability_id") === abilityId,
    "Selected sample ability_id mismatch");
  assert(integerNumber(sample.hit_event_id, "sample.hit_event_id") === hitEventId,
    "Selected sample hit_event_id mismatch");
  assert(integerNumber(sample.amount, "sample.amount") === selectedPacketFinal,
    "Selected sample packet final mismatch");
  assert(sample.lucky === false, "Initial mismatch harness supports packet-proven non-lucky hits only");

  const damageRule = only(damageStage.rules.filter((entry) =>
    integerNumber(entry.ability_id, "damage rule ability_id") === abilityId &&
    integerNumber(entry.hit_event_id, "damage rule hit_event_id") === hitEventId &&
    integerNumber(entry.damage_attr_id, "damage rule damage_attr_id") === damageAttrId),
  "damage-stage rule");
  const coefficient = integer(seed.model?.coefficient_basis_points, "coefficient_basis_points");
  safeJsonInteger(coefficient);
  assert((damageRule.coefficient_basis_points_by_stage ?? []).some((value) =>
    integer(value, "damage rule coefficient basis points") === coefficient),
    "Seed coefficient is not declared by the exact damage-stage rule");
  const fixedParameter = integer(seed.model?.flat_damage ?? 0, "flat_damage");
  safeJsonInteger(fixedParameter);
  if ((damageRule.fixed_parameter_by_level ?? []).length === 0) {
    assert(fixedParameter === 0n, "Damage-stage rule has no fixed ladder but seed flat_damage is nonzero");
  } else {
    assert((damageRule.fixed_parameter_by_level ?? []).some((value) =>
      integer(value, "damage rule fixed parameter") === fixedParameter),
      "Seed flat_damage is not declared by the exact damage-stage rule");
  }

  const sourceAttributeStateId = integerNumber(sample.source_attribute_state_id,
    "sample.source_attribute_state_id");
  const targetAttributeStateId = integerNumber(sample.target_attribute_state_id,
    "sample.target_attribute_state_id");
  const sourceState = only((context.source_attribute_states ?? []).filter((entry) =>
    integerNumber(entry.state_id, "source attribute state_id") === sourceAttributeStateId),
  "source attribute state");
  const targetState = only((context.target_attribute_states ?? []).filter((entry) =>
    integerNumber(entry.state_id, "target attribute state_id") === targetAttributeStateId),
  "target attribute state");
  const effectiveAttack = attributeValue(sourceState.attributes,
    integerNumber(seed.model?.effective_attack_source_attribute_id, "effective_attack_source_attribute_id"));
  const targetDefense = attributeValue(targetState.attributes,
    integerNumber(seed.model?.target_defense_attribute_id, "target_defense_attribute_id"));
  const defenseScaler = integer(seed.model?.target_defense_scaler, "target_defense_scaler");
  assert(defenseScaler > 0n, "target_defense_scaler must be positive");
  const defenseFreeAttack = integer(seed.model?.defense_free_attack ?? 0, "defense_free_attack");
  assert(defenseFreeAttack >= 0n, "defense_free_attack must be nonnegative");

  const bucketBasisPoints = {};
  for (const stage of MULTIPLIER_STAGES) {
    bucketBasisPoints[stage] = integer(seed.model?.bucket_basis_points?.[stage] ?? 0,
      `bucket_basis_points.${stage}`);
    assert(bucketBasisPoints[stage] > -BASIS_POINT_SCALE,
      `bucket_basis_points.${stage} must keep a positive multiplier`);
  }
  const criticalMultiplier = sample.critical === true
    ? resolveCriticalMultiplier(seed.model?.critical, sourceState.attributes)
    : rational(1n, 1n);
  const rounding = validateRounding(seed.model?.rounding);

  const model = {
    effectiveAttack: BigInt(effectiveAttack),
    targetDefense: BigInt(targetDefense),
    defenseScaler,
    defenseFreeAttack,
    coefficient,
    fixedParameter,
    bucketBasisPoints,
    criticalMultiplier,
    critical: sample.critical === true,
    rounding,
  };
  const baseline = evaluate(model);
  const packetFinal = BigInt(selectedPacketFinal);
  const mismatch = mismatchRow(packetFinal, baseline, rounding.packet_final);

  const seenDedup = new Set();
  const candidateStageCorrections = [];
  for (const candidate of seed.candidate_corrections ?? []) {
    validateCandidate(candidate, ledger, seenDedup);
    const correctedBuckets = { ...bucketBasisPoints };
    correctedBuckets[candidate.stage] += integer(candidate.basis_points,
      `candidate ${candidate.key} basis_points`);
    assert(correctedBuckets[candidate.stage] > -BASIS_POINT_SCALE,
      `Candidate ${candidate.key} makes its multiplier nonpositive`);
    const corrected = evaluate({ ...model, bucketBasisPoints: correctedBuckets });
    const correctedMismatch = mismatchRow(packetFinal, corrected, rounding.packet_final);
    if (BigInt(correctedMismatch.residual_abs) >= BigInt(mismatch.residual_abs)) continue;
    candidateStageCorrections.push({
      key: candidate.key,
      stage: candidate.stage,
      operation: "additive_basis_points",
      basis_points: safeJsonInteger(integer(candidate.basis_points,
        `candidate ${candidate.key} basis_points`)),
      dedup_key: candidate.dedup_key,
      ledger_pointer: candidate.ledger_pointer,
      corrected_prediction: correctedMismatch.predicted_integer,
      corrected_residual_signed: correctedMismatch.residual_signed,
      corrected_residual_abs: correctedMismatch.residual_abs,
      exact_match: correctedMismatch.residual_signed === 0,
      provenance: candidate.provenance,
      formula_authority: false,
    });
  }
  candidateStageCorrections.sort((left, right) =>
    left.corrected_residual_abs - right.corrected_residual_abs || compareText(left.key, right.key));

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: seed.game_build,
    policy: {
      exact_numeric_ids_and_build_identity_authoritative: true,
      doma_stage_order_is_hypothesis_only: true,
      calculator_authority: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      arbitrary_residual_to_mechanic_inference_allowed: false,
      canonical_packet_damage_preserved: true,
    },
    identity: {
      session_id: sample.session_id,
      sequence: sample.sequence,
      ability_id: sample.ability_id,
      hit_event_id: sample.hit_event_id,
      damage_attr_id: damageAttrId,
      source_entity_uuid: sample.source_entity_uuid,
      target_entity_uuid: sample.target_entity_uuid,
      critical: sample.critical,
      lucky: sample.lucky,
    },
    inputs: {
      seed: descriptor(seedPath),
      sample_context: descriptor(contextPath),
      damage_stage: descriptor(damageStagePath),
      candidate_ledger: descriptor(ledgerPath),
    },
    model_receipt: {
      order: [
        "effective attack x (1 - target resistance)",
        "+ defense-free attack",
        "x exact skill coefficient + exact flat damage",
        ...MULTIPLIER_STAGES.map((stage) => `x ${stage}`),
        "x critical multiplier when packet critical",
        "packet-final integer boundary",
      ],
      exact_coefficient_basis_points: safeJsonInteger(coefficient),
      packet_attribute_inputs: {
        effective_attack_attribute_id: integerNumber(seed.model.effective_attack_source_attribute_id,
          "effective_attack_source_attribute_id"),
        target_defense_attribute_id: integerNumber(seed.model.target_defense_attribute_id,
          "target_defense_attribute_id"),
      },
      rounding,
      critical: {
        packet_critical: sample.critical,
        configured_mode: sample.critical ? seed.model.critical.mode : null,
        exact_multiplier_numerator: safeJsonInteger(criticalMultiplier.numerator),
        exact_multiplier_denominator: safeJsonInteger(criticalMultiplier.denominator),
      },
      model_is_runtime_authority: false,
    },
    mismatch,
    candidate_stage_corrections: candidateStageCorrections,
    content_sha256: null,
  };
}

function evaluate(model) {
  let value = rational(model.effectiveAttack * model.defenseScaler,
    model.targetDefense + model.defenseScaler);
  value = boundary(value, model.rounding.attack_after_resistance);
  value = add(value, rational(model.defenseFreeAttack, 1n));
  value = add(multiply(value, rational(model.coefficient, BASIS_POINT_SCALE)),
    rational(model.fixedParameter, 1n));
  value = boundary(value, model.rounding.skill_base);
  for (const stage of MULTIPLIER_STAGES) {
    value = multiply(value, rational(BASIS_POINT_SCALE + model.bucketBasisPoints[stage],
      BASIS_POINT_SCALE));
    value = boundary(value, model.rounding[stage]);
  }
  if (model.critical) {
    value = multiply(value, model.criticalMultiplier);
  }
  value = boundary(value, model.rounding.critical);
  value = boundary(value, model.rounding.packet_final);
  assert(value.denominator === 1n, "packet_final rounding must produce an integer");
  return value.numerator;
}

function mismatchRow(packetFinal, predicted, roundingModel) {
  const residual = packetFinal - predicted;
  const absolute = residual < 0n ? -residual : residual;
  return {
    packet_final_integer: safeJsonInteger(packetFinal),
    predicted_integer: safeJsonInteger(predicted),
    residual_signed: safeJsonInteger(residual),
    residual_abs: safeJsonInteger(absolute),
    residual_ppm: packetFinal === 0n ? null :
      safeJsonInteger((absolute * 1_000_000n) / packetFinal),
    packet_final_rounding_model: roundingModel,
  };
}

function validateCandidate(candidate, ledger, seenDedup) {
  assert(candidate && typeof candidate.key === "string" && candidate.key.length > 0,
    "Candidate key is required");
  assert(MULTIPLIER_STAGES.includes(candidate.stage),
    `Candidate ${candidate.key} has unsupported stage ${candidate.stage}`);
  assert(candidate.operation === "additive_basis_points",
    `Candidate ${candidate.key} must use additive_basis_points`);
  assert(typeof candidate.dedup_key === "string" && candidate.dedup_key.length > 0,
    `Candidate ${candidate.key} dedup_key is required`);
  assert(!seenDedup.has(candidate.dedup_key),
    `Duplicate candidate dedup_key ${candidate.dedup_key}`);
  seenDedup.add(candidate.dedup_key);
  assert(typeof candidate.ledger_pointer === "string" && candidate.ledger_pointer.startsWith("/"),
    `Candidate ${candidate.key} ledger_pointer is required`);
  const ledgerValue = jsonPointer(ledger, candidate.ledger_pointer);
  assert(ledgerValue && typeof ledgerValue === "object", `Candidate ${candidate.key} ledger pointer is missing`);
  assert(String(ledgerValue.key) === candidate.key,
    `Candidate ${candidate.key} does not match ledger key at ${candidate.ledger_pointer}`);
  const basisPoints = integer(candidate.basis_points, `Candidate ${candidate.key} basis_points`);
  const ledgerBasisPoints = integer(ledgerValue.candidate_basis_points,
    `Candidate ${candidate.key} ledger candidate_basis_points`);
  safeJsonInteger(basisPoints);
  assert(ledgerBasisPoints === basisPoints,
    `Candidate ${candidate.key} basis points do not match its ledger row`);
  assert(candidate.provenance && typeof candidate.provenance.path === "string" &&
    candidate.provenance.path.length > 0,
  `Candidate ${candidate.key} provenance path is required`);
}

function validateRounding(value) {
  assert(value && typeof value === "object", "Explicit rounding map is required");
  const output = {};
  for (const stage of ROUNDING_STAGES) {
    const mode = value[stage];
    assert(ROUNDING_MODES.has(mode), `Unsupported or missing rounding mode for ${stage}`);
    output[stage] = mode;
  }
  assert(output.packet_final !== "defer", "packet_final rounding cannot be defer");
  return output;
}

function resolveCriticalMultiplier(value, attributes) {
  assert(value && typeof value === "object", "Critical packet requires an explicit critical model");
  if (value.mode === "source_total_multiplier_attribute") {
    const total = BigInt(attributeValue(attributes,
      integerNumber(value.source_attribute_id, "critical.source_attribute_id")));
    assert(total > 0n, "Critical total multiplier attribute must be positive");
    return rational(total, BASIS_POINT_SCALE);
  }
  if (value.mode === "literal_total_multiplier_basis_points") {
    const total = integer(value.basis_points, "critical.basis_points");
    assert(total > 0n, "Critical total multiplier must be positive");
    return rational(total, BASIS_POINT_SCALE);
  }
  if (value.mode === "source_bonus_attribute") {
    const bonus = BigInt(attributeValue(attributes,
      integerNumber(value.source_attribute_id, "critical.source_attribute_id")));
    assert(bonus > -BASIS_POINT_SCALE, "Critical bonus must keep a positive multiplier");
    return rational(BASIS_POINT_SCALE + bonus, BASIS_POINT_SCALE);
  }
  if (value.mode === "literal_bonus_basis_points") {
    const bonus = integer(value.basis_points, "critical.basis_points");
    assert(bonus > -BASIS_POINT_SCALE, "Critical bonus must keep a positive multiplier");
    return rational(BASIS_POINT_SCALE + bonus, BASIS_POINT_SCALE);
  }
  throw new Error(`Unsupported critical mode ${value.mode}`);
}

function boundary(value, mode) {
  if (mode === "defer") return value;
  const quotient = value.numerator / value.denominator;
  const remainder = value.numerator % value.denominator;
  if (mode === "floor") return rational(quotient, 1n);
  if (mode === "ceil") return rational(quotient + (remainder === 0n ? 0n : 1n), 1n);
  if (mode === "positive_half_up") {
    return rational(quotient + (remainder * 2n >= value.denominator ? 1n : 0n), 1n);
  }
  throw new Error(`Unsupported rounding mode ${mode}`);
}

function rational(numerator, denominator) {
  assert(denominator > 0n, "Rational denominator must be positive");
  assert(numerator >= 0n, "Damage hypothesis intermediates must be nonnegative");
  const divisor = gcd(numerator, denominator);
  return { numerator: numerator / divisor, denominator: denominator / divisor };
}

function add(left, right) {
  return rational(left.numerator * right.denominator + right.numerator * left.denominator,
    left.denominator * right.denominator);
}

function multiply(left, right) {
  return rational(left.numerator * right.numerator, left.denominator * right.denominator);
}

function gcd(left, right) {
  let a = left < 0n ? -left : left;
  let b = right < 0n ? -right : right;
  while (b !== 0n) [a, b] = [b, a % b];
  return a === 0n ? 1n : a;
}

function verifyCommand(parsed) {
  const input = resolved(required(parsed, "input"));
  const report = readBoundedJson(input, "mismatch report");
  verifyReport(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  assert(report?.schema_version === SCHEMA_VERSION && report?.generated_by === GENERATOR,
    "Mismatch report identity is invalid");
  assert(report?.content_sha256 === contentHash(report), "Mismatch report content hash mismatch");
  assert(report?.policy?.calculator_authority === false &&
    report?.policy?.formula_authority === false &&
    report?.policy?.runtime_authority === false &&
    report?.policy?.provider_rdps_credit_allowed === false &&
    report?.policy?.canonical_packet_damage_preserved === true,
  "Mismatch report unexpectedly grants authority or changes canonical damage");
  assert(Number.isSafeInteger(report?.mismatch?.packet_final_integer) &&
    Number.isSafeInteger(report?.mismatch?.predicted_integer) &&
    Number.isSafeInteger(report?.mismatch?.residual_signed) &&
    Number.isSafeInteger(report?.mismatch?.residual_abs),
  "Mismatch report integer fields are invalid");
  assert(report.mismatch.packet_final_integer - report.mismatch.predicted_integer ===
    report.mismatch.residual_signed, "Mismatch residual is inconsistent");
  assert(Math.abs(report.mismatch.residual_signed) === report.mismatch.residual_abs,
    "Mismatch absolute residual is inconsistent");
  const dedup = new Set();
  for (const candidate of report.candidate_stage_corrections ?? []) {
    assert(candidate.formula_authority === false && candidate.corrected_residual_abs <
      report.mismatch.residual_abs, "Candidate correction does not improve the mismatch fail-closed");
    assert(!dedup.has(candidate.dedup_key), "Duplicate emitted candidate correction");
    dedup.add(candidate.dedup_key);
  }
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-packet-final-mismatch-"));
  try {
    const contextPath = path.join(root, "context.json");
    const damageStagePath = path.join(root, "damage-stage.json");
    const ledgerPath = path.join(root, "ledger.json");
    const seedPath = path.join(root, "seed.json");
    const outputPath = path.join(root, "output.json");
    writeJson(contextPath, {
      schema_version: 2,
      generated_by: "tools/bpsr-selected-pair-formula-context.mjs",
      game_build: "test-build",
      selected_samples: [{
        session_id: "test.run-0001", sequence: 7, ability_id: 100, hit_event_id: 3,
        amount: 193, critical: false, lucky: false, source_entity_uuid: 1,
        target_entity_uuid: 2, source_attribute_state_id: 10, target_attribute_state_id: 20,
      }],
      source_attribute_states: [{ state_id: 10, attributes: [{ attribute_id: 11330, value: 100 }] }],
      target_attribute_states: [{ state_id: 20, attributes: [{ attribute_id: 11440, value: 0 }] }],
    });
    writeJson(damageStagePath, {
      schema_version: 1, game_build: "test-build",
      rules: [{ ability_id: 100, hit_event_id: 3, damage_attr_id: 10003,
        coefficient_basis_points_by_stage: [20_000], fixed_parameter_by_level: [] }],
    });
    writeJson(ledgerPath, {
      schema_version: 7, game_build: "test-build",
      fixed_component_candidates: [
        { key: "wrong", candidate_basis_points: 100 },
        { key: "missing-ten-percent", candidate_basis_points: 1_000 },
      ],
    });
    const rounding = Object.fromEntries(ROUNDING_STAGES.map((stage) =>
      [stage, stage === "packet_final" ? "floor" : "defer"]));
    writeJson(seedPath, {
      schema_version: 1,
      game_build: "test-build",
      sample_context: { path: "context.json" },
      damage_stage: { path: "damage-stage.json" },
      candidate_ledger: { path: "ledger.json" },
      selection: { sequence: 7, ability_id: 100, hit_event_id: 3,
        damage_attr_id: 10003, packet_final_integer: 193 },
      model: {
        effective_attack_source_attribute_id: 11330,
        target_defense_attribute_id: 11440,
        target_defense_scaler: 6500,
        defense_free_attack: 0,
        coefficient_basis_points: 20_000,
        flat_damage: 0,
        bucket_basis_points: {},
        rounding,
      },
      candidate_corrections: [
        { key: "wrong", stage: "generic", operation: "additive_basis_points",
          basis_points: 100, dedup_key: "wrong", ledger_pointer: "/fixed_component_candidates/0",
          provenance: { path: "ledger.json" } },
        { key: "missing-ten-percent", stage: "generic", operation: "additive_basis_points",
          basis_points: 1_000, dedup_key: "ten-percent",
          ledger_pointer: "/fixed_component_candidates/1", provenance: { path: "ledger.json" } },
      ],
    });
    const report = buildReport(seedPath);
    report.content_sha256 = contentHash(report);
    verifyReport(report);
    assert(report.mismatch.predicted_integer === 200 && report.mismatch.residual_signed === -7,
      "Self-test baseline mismatch changed");
    assert(report.candidate_stage_corrections.length === 0,
      "Self-test must suppress candidates that do not improve a negative residual");

    const exact = boundary(rational(5n, 2n), "positive_half_up");
    assert(exact.numerator === 3n && boundary(rational(5n, 2n), "floor").numerator === 2n &&
      boundary(rational(5n, 2n), "ceil").numerator === 3n,
    "Self-test rounding modes changed");
    const totalCritical = resolveCriticalMultiplier({
      mode: "source_total_multiplier_attribute", source_attribute_id: 12510,
    }, [{ attribute_id: 12510, value: 22206 }]);
    assert(totalCritical.numerator === 11103n && totalCritical.denominator === 5000n,
      "Self-test total critical multiplier interpretation changed");

    const seed = readBoundedJson(seedPath, "self-test seed");
    const context = readBoundedJson(contextPath, "self-test context");
    context.selected_samples[0].amount = 220;
    writeJson(contextPath, context);
    seed.selection.packet_final_integer = 220;
    writeJson(seedPath, seed);
    const correctedReport = buildReport(seedPath);
    correctedReport.content_sha256 = contentHash(correctedReport);
    verifyReport(correctedReport);
    assert(correctedReport.mismatch.residual_signed === 20 &&
      correctedReport.candidate_stage_corrections.length === 2 &&
      correctedReport.candidate_stage_corrections[0].key === "missing-ten-percent" &&
      correctedReport.candidate_stage_corrections[0].exact_match === true,
    "Self-test candidate correction ranking changed");
    writeJson(outputPath, correctedReport);
    verifyReport(readBoundedJson(outputPath, "self-test output"));
    console.log("bpsr-packet-final-mismatch-harness self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function jsonPointer(value, pointer) {
  if (pointer === "") return value;
  return pointer.slice(1).split("/").reduce((current, segment) => {
    const key = segment.replaceAll("~1", "/").replaceAll("~0", "~");
    return current?.[key];
  }, value);
}

function attributeValue(attributes, id) {
  const row = only((attributes ?? []).filter((entry) =>
    integerNumber(entry.attribute_id, "attribute_id") === id),
    `attribute ${id}`);
  return integerNumber(row.value, `attribute ${id} value`);
}

function descriptor(file) {
  return {
    path: file.replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: `sha256:${hashFile(file)}`,
  };
}

function sourcePath(directory, value, label) {
  assert(typeof value === "string" && value.length > 0, `${label} path is required`);
  const result = path.resolve(directory, value);
  assert(existsSync(result) && statSync(result).isFile(), `Missing ${label}: ${result}`);
  return result;
}

function readBoundedJson(file, label) {
  const size = statSync(file).size;
  assert(size <= MAX_INPUT_BYTES, `${label} exceeds bounded ${MAX_INPUT_BYTES}-byte input limit`);
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`Unable to read ${label} ${file}: ${error.message}`);
  }
}

function requireBuild(value, build, label) {
  assert(String(value?.game_build) === String(build), `${label} build identity mismatch`);
}

function integer(value, label) {
  if (typeof value === "bigint") return value;
  if (typeof value === "number") {
    assert(Number.isSafeInteger(value), `${label} must be a safe integer`);
    return BigInt(value);
  }
  if (typeof value === "string" && /^-?\d+$/.test(value)) return BigInt(value);
  throw new Error(`${label} must be an integer`);
}

function integerNumber(value, label) {
  const result = Number(value);
  assert(Number.isSafeInteger(result), `${label} must be a safe integer`);
  return result;
}

function safeJsonInteger(value) {
  const number = Number(value);
  assert(Number.isSafeInteger(number), "Output integer exceeds JSON safe range");
  return number;
}

function only(values, label) {
  assert(values.length === 1, `Expected exactly one ${label}, found ${values.length}`);
  return values[0];
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return `sha256:${createHash("sha256").update(stableStringify(clone)).digest("hex")}`;
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function hashFile(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function writeJson(file, value) {
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`);
    const key = arg.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function required(value, key) {
  if (!value[key]) throw new Error(`Missing --${key}`);
  return value[key];
}

function resolved(value) { return path.resolve(value); }
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function assert(condition, message) { if (!condition) throw new Error(message); }

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-packet-final-mismatch-harness.mjs build --seed <json> --output <json>\n  node tools/bpsr-packet-final-mismatch-harness.mjs verify --input <json>\n  node tools/bpsr-packet-final-mismatch-harness.mjs self-test");
  process.exit(exitCode);
}
