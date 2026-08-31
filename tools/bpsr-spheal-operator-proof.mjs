#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 2;
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "analyze") analyzeCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyzeCommand(parsed) {
  const healingProofPath = path.resolve(required(parsed, "healing-proof"));
  const exactEffectOccurrenceProofPath = path.resolve(required(parsed, "exact-effect-occurrence-proof"));
  const damageAttrTablePath = path.resolve(required(parsed, "damage-attr-table"));
  const formulaSurfacePath = path.resolve(required(parsed, "damage-formula-surface"));
  const output = path.resolve(required(parsed, "output"));
  const effectId = positiveInteger(required(parsed, "effect"), "effect");
  const healingProof = readJson(healingProofPath, "state-scaling healing proof");
  const exactEffectOccurrenceProof = readJson(
    exactEffectOccurrenceProofPath,
    "exact-effect occurrence proof",
  );
  const damageAttrTable = readJson(damageAttrTablePath, "DamageAttrTable");
  const formulaSurface = readJson(formulaSurfacePath, "damage formula surface");
  const damageAttrInput = fileDescriptor(damageAttrTablePath);
  const staticRows = validateStaticInputs(
    damageAttrTable,
    formulaSurface,
    damageAttrInput,
    String(healingProof.game_build ?? ""),
  );
  const report = analyzeOperator(healingProof, staticRows, {
    effectId,
    healingProofInput: fileDescriptor(healingProofPath),
    exactEffectOccurrenceProof,
    exactEffectOccurrenceProofInput: fileDescriptor(exactEffectOccurrenceProofPath),
    damageAttrInput,
    formulaSurfaceInput: fileDescriptor(formulaSurfacePath),
  });
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(`wrote ${output}`);
}

function analyzeOperator(healingProof, staticRows, context) {
  validateHealingProof(healingProof);
  const gameBuild = String(healingProof.game_build);
  const exactEffectOccurrenceEvidence = validateExactEffectOccurrenceProof(
    context.exactEffectOccurrenceProof,
    context.exactEffectOccurrenceProofInput,
    context.effectId,
    gameBuild,
    healingProof.inputs,
  );
  const spHealRows = staticRows.filter((row) => row.damage_script === "SpHeal");
  const effectRows = staticRows.filter((row) => row.type_enum === context.effectId);
  const exactEffectSpHealRows = effectRows.filter((row) => row.damage_script === "SpHeal");
  if (exactEffectSpHealRows.length === 0) {
    throw new Error(`Exact effect ${context.effectId} has no SpHeal output row`);
  }
  const families = (healingProof.formula_families ?? []).map((entry, index) =>
    normalizeObservedFamily(entry, index, spHealRows)
  );
  const observedEffectFamilies = families.filter((entry) => entry.family.ability_id === context.effectId);
  const candidateSets = families.map((entry) => new Set(
    entry.full_coverage_reported_amount_hp_candidates.map((candidate) => candidate.basis_points),
  ));
  const commonBasisPoints = candidateSets.length === 0
    ? []
    : [...candidateSets[0]].filter((candidate) =>
      candidateSets.slice(1).every((set) => set.has(candidate))
    ).sort((left, right) => left - right);
  const distinctBasisPoints = [...new Set(families.flatMap((entry) =>
    entry.full_coverage_reported_amount_hp_candidates.map((candidate) => candidate.basis_points)
  ))].sort((left, right) => left - right);
  const familiesWithCandidate = families.filter((entry) =>
    entry.full_coverage_reported_amount_hp_candidates.length > 0
  ).length;
  const observedStaticRowIds = [...new Set(families.flatMap((entry) =>
    entry.static_row_candidates.map((row) => row.damage_attr_id)
  ))].sort((left, right) => left - right);
  return {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-spheal-operator-proof.mjs",
    game_build: gameBuild,
    effect_id: context.effectId,
    policy: {
      exact_numeric_effect_id_is_authoritative: true,
      exact_input_build_and_hashes_are_authoritative: true,
      damage_script_name_is_grouping_evidence_not_formula_authority: true,
      localized_names_are_evidence_only: true,
      packet_absence_is_not_zero: true,
      candidate_hp_ratios_are_compatibility_constraints_not_operator_proof: true,
      unobserved_effect_rows_are_never_backfilled_from_other_spheal_rows: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      unresolved_evidence_is_preserved: true,
    },
    inputs: {
      healing_state_proof: context.healingProofInput,
      exact_effect_occurrence_proof: context.exactEffectOccurrenceProofInput,
      damage_attr_table: context.damageAttrInput,
      exact_build_formula_surface: context.formulaSurfaceInput,
      rlogs: structuredClone(healingProof.inputs),
      exact_effect_occurrence_rlogs: structuredClone(exactEffectOccurrenceEvidence.input_rlogs),
    },
    exact_effect_static_rows: effectRows,
    exact_effect_spheal_rows: exactEffectSpHealRows,
    summary: {
      selected_spheal_type_ids: new Set(spHealRows.map((row) => row.type_enum)).size,
      packet_observed_spheal_formula_families: families.length,
      packet_observed_spheal_type_ids: new Set(families.map((entry) => entry.family.ability_id)).size,
      packet_observed_spheal_static_rows: observedStaticRowIds.length,
      packet_observed_exact_effect_formula_families: observedEffectFamilies.length,
      exact_effect_occurrence_proof_rlogs: exactEffectOccurrenceEvidence.input_rlogs.length,
      exact_effect_occurrence_proof_healing_events_scanned:
        exactEffectOccurrenceEvidence.summary.healing_events_scanned,
      exact_effect_occurrence_proof_selected_events:
        exactEffectOccurrenceEvidence.summary.healing_events_selected,
      families_with_full_coverage_hp_candidate: familiesWithCandidate,
      families_without_full_coverage_hp_candidate: families.length - familiesWithCandidate,
      distinct_full_coverage_hp_basis_points: distinctBasisPoints,
      common_full_coverage_hp_basis_points_across_every_family: commonBasisPoints,
      spheal_family_wide_single_hp_ratio_proven:
        families.length > 0 && familiesWithCandidate === families.length && commonBasisPoints.length === 1,
      exact_effect_output_packet_observed: observedEffectFamilies.length > 0,
      exact_effect_spheal_coefficient_to_hp_basis_binding_proven: false,
      damage_script_identity_alone_proves_operator: false,
      current_hp_vs_max_hp_basis_disambiguated: false,
      exact_effect_operator_proven: false,
    },
    observed_formula_families: families,
    interpretation: {
      exact_effect_output_occurrence_missing: observedEffectFamilies.length === 0,
      exact_effect_output_absent_in_all_complete_matching_build_capture_inputs:
        exactEffectOccurrenceEvidence.summary.healing_events_selected === 0,
      heterogeneous_spheal_family_evidence:
        distinctBasisPoints.length > 1 || familiesWithCandidate !== families.length,
      family_name_transfer_to_exact_effect_allowed: false,
      exact_effect_formula_authority: false,
      exact_effect_runtime_authority: false,
      provider_rdps_credit_allowed: false,
      next_required_evidence: [
        `same-build packet occurrence for exact effect ${context.effectId} output row`,
        "exact calculation-time source and target state for that output",
        "isolated input delta for the exact row and level or stage",
        "effect-output magnitude to applied-status transform binding",
        "operation order and integer rounding discrimination",
        "canonical counterfactual replay conservation",
      ],
    },
  };
}

function validateHealingProof(proof) {
  if (![4, 5].includes(proof?.schema_version) ||
    proof?.generated_by !== "rlogs-bpsr-state-scaling-healing-proof" ||
    !/^\d+$/.test(String(proof?.game_build ?? "")) ||
    proof?.policy?.exact_input_build_is_authoritative !== true ||
    proof?.policy?.exact_input_hashes_are_embedded !== true ||
    proof?.policy?.healing_events_are_discarded !== false ||
    proof?.policy?.unresolved_hp_formulas_are_hidden !== false ||
    !Array.isArray(proof.inputs) || proof.inputs.length === 0 ||
    proof.inputs.some((input) =>
      String(input.game_build ?? "") !== String(proof.game_build) ||
      !Number.isSafeInteger(Number(input.bytes)) || Number(input.bytes) <= 0 ||
      !/^sha256:[0-9a-f]{64}$/.test(String(input.sha256 ?? "")) ||
      !String(input.path ?? "")
    ) || new Set(proof.inputs.map((input) => input.path)).size !== proof.inputs.length ||
    JSON.stringify(proof.source_rlogs ?? []) !== JSON.stringify(proof.inputs.map((input) => input.path)) ||
    !Array.isArray(proof.formula_families)) {
    throw new Error("State-scaling healing proof is not build-locked schema-4 evidence");
  }
}

function validateExactEffectOccurrenceProof(proof, proofInput, effectId, build, requiredInputs) {
  validateHealingProof(proof);
  if (proof.schema_version !== 5 ||
    String(proof.game_build) !== build ||
    proof.selection?.all_abilities !== false ||
    JSON.stringify(proof.selection?.ability_ids) !== JSON.stringify([effectId]) ||
    Number(proof.summary?.healing_events_scanned) < 0 ||
    proof.summary?.healing_events_selected !== 0 ||
    proof.summary?.selected_ability_ids !== 0 ||
    proof.summary?.selected_formula_families !== 0 ||
    Number(proof.summary?.selected_amount_sum) !== 0 ||
    proof.formula_families.length !== 0) {
    throw new Error("Exact-effect occurrence proof is not a zero-occurrence schema-5 exact-ID scan");
  }
  const occurrenceInputs = new Map(proof.inputs.map((input) => [normalizedInputPath(input.path), input]));
  for (const requiredInput of requiredInputs) {
    const observed = occurrenceInputs.get(normalizedInputPath(requiredInput.path));
    if (!observed || Number(observed.bytes) !== Number(requiredInput.bytes) ||
      observed.sha256 !== requiredInput.sha256 || observed.game_build !== requiredInput.game_build) {
      throw new Error("Exact-effect occurrence proof does not contain every formula-family input identity");
    }
  }
  return {
    proof: proofInput,
    selection: structuredClone(proof.selection),
    summary: structuredClone(proof.summary),
    input_rlogs: structuredClone(proof.inputs),
  };
}

function normalizedInputPath(value) {
  return path.resolve(String(value)).replaceAll("\\", "/").toLowerCase();
}

function validateStaticInputs(table, surface, tableInput, build) {
  if (!table || Array.isArray(table) || typeof table !== "object" ||
    surface?.schema_version !== 1 ||
    surface?.generated_by !== "rlogs-bpsr-damage-attr-semantic-surface" ||
    String(surface?.game_build ?? "") !== build ||
    surface?.policy?.runtime_formula_authority !== false ||
    surface?.policy?.semantic_decoded_bridge !== true ||
    surface?.policy?.exact_build_table_required !== true ||
    surface?.policy?.unresolved_rows_hidden !== false ||
    Number(surface?.input?.bytes) !== tableInput.bytes ||
    String(surface?.input?.sha256 ?? "") !== tableInput.sha256) {
    throw new Error("DamageAttrTable is not bound to the exact-build semantic surface");
  }
  return Object.entries(table).map(([key, row]) => {
    const damageAttrId = positiveInteger(row?.Id, `DamageAttrTable row ${key} Id`);
    if (String(damageAttrId) !== key) throw new Error(`DamageAttrTable row key ${key} is inconsistent`);
    const typeEnum = positiveInteger(row?.TypeEnum, `DamageAttrTable row ${key} TypeEnum`);
    const semantic = surface.rows?.[key];
    const coefficient = exactIntegerArray(row.PVEDamageRadio, `DamageAttrTable row ${key} PVEDamageRadio`);
    const fixed = exactIntegerArray(row.PVEFixedParameter, `DamageAttrTable row ${key} PVEFixedParameter`);
    if (Number(semantic?.damage_id) !== damageAttrId || Number(semantic?.linked_id) !== typeEnum ||
      JSON.stringify(semantic?.int_array_pool_1_candidates_by_offset?.["28"]?.values) !==
        JSON.stringify(coefficient) ||
      JSON.stringify(semantic?.int_array_pool_1_candidates_by_offset?.["32"]?.values) !==
        JSON.stringify(fixed)) {
      throw new Error(`DamageAttrTable row ${key} does not match the exact-build semantic surface`);
    }
    return {
      damage_attr_id: damageAttrId,
      type_enum: typeEnum,
      hit_event_suffix_candidate: damageAttrId % 100,
      damage_script: typeof row.DamageScript === "string" ? row.DamageScript : null,
      coefficient_basis_points_by_stage: coefficient,
      fixed_parameter_by_level: fixed,
      pve_loop_time: exactInteger(row.PVELoopTime, `DamageAttrTable row ${key} PVELoopTime`),
      damage_type: exactInteger(row.DamageType, `DamageAttrTable row ${key} DamageType`),
      row_level: exactInteger(row.Level, `DamageAttrTable row ${key} Level`),
    };
  });
}

function normalizeObservedFamily(entry, index, spHealRows) {
  const family = entry?.family;
  const abilityId = positiveInteger(family?.ability_id, `healing family ${index} ability ID`);
  const hitEventId = exactNonNegativeInteger(
    family?.hit_event_id,
    `healing family ${index} hit event ID`,
  );
  const staticRowCandidates = spHealRows.filter((row) =>
    row.type_enum === abilityId && row.hit_event_suffix_candidate === hitEventId
  );
  if (staticRowCandidates.length === 0) {
    throw new Error(`Healing family ${index} has no exact SpHeal TypeEnum and hit-suffix row`);
  }
  const candidates = [];
  for (const basis of entry.reported_amount_candidates ?? []) {
    for (const candidate of basis.candidates ?? []) {
      if (Number(candidate.coverage_basis_points) === 10_000) {
        candidates.push({
          basis: String(basis.basis),
          basis_points: exactNonNegativeInteger(
            candidate.basis_points,
            `healing family ${index} basis points`,
          ),
          events: exactNonNegativeInteger(candidate.events, `healing family ${index} candidate events`),
          distinct_numerators: exactNonNegativeInteger(
            candidate.distinct_numerators,
            `healing family ${index} candidate numerators`,
          ),
          distinct_denominators: exactNonNegativeInteger(
            candidate.distinct_denominators,
            `healing family ${index} candidate denominators`,
          ),
        });
      }
    }
  }
  return {
    family: {
      ability_id: abilityId,
      hit_event_id: hitEventId,
      damage_source: family.damage_source ?? null,
      damage_type: family.damage_type ?? null,
      damage_mode: family.damage_mode ?? null,
      owner_level: family.owner_level ?? null,
      owner_stage: family.owner_stage ?? null,
      source_entity_uuid: family.raw_attacker_uuid ?? null,
    },
    events: exactNonNegativeInteger(entry.events, `healing family ${index} events`),
    amount_min: exactInteger(entry.amount_min, `healing family ${index} minimum amount`),
    amount_max: exactInteger(entry.amount_max, `healing family ${index} maximum amount`),
    self_target_events: exactNonNegativeInteger(
      entry.self_target_events,
      `healing family ${index} self-target events`,
    ),
    source_entities: exactPositiveIntegerArray(
      entry.source_entities,
      `healing family ${index} source entities`,
    ),
    target_entity_count: Array.isArray(entry.target_entities) ? entry.target_entities.length : 0,
    static_row_resolution: staticRowCandidates.length === 1
      ? "unique-type-enum-and-hit-suffix-candidate"
      : "ambiguous-type-enum-and-hit-suffix-candidates",
    static_row_candidates: structuredClone(staticRowCandidates),
    full_coverage_reported_amount_hp_candidates: candidates,
    current_hp_vs_max_hp_basis_disambiguated: false,
    formula_authority: false,
    runtime_authority: false,
  };
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

function exactIntegerArray(value, label) {
  if (!Array.isArray(value) || value.some((entry) => !Number.isSafeInteger(Number(entry)))) {
    throw new Error(`${label} must be an exact integer array`);
  }
  return value.map(Number);
}

function exactPositiveIntegerArray(value, label) {
  if (!Array.isArray(value) || value.length === 0 ||
    value.some((entry) => !Number.isSafeInteger(Number(entry)) || Number(entry) <= 0)) {
    throw new Error(`${label} must be a non-empty exact positive integer array`);
  }
  return value.map(Number);
}

function exactInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) throw new Error(`${label} must be an exact integer`);
  return parsed;
}

function exactNonNegativeInteger(value, label) {
  const parsed = exactInteger(value, label);
  if (parsed < 0) throw new Error(`${label} must be non-negative`);
  return parsed;
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
  const input = {
    schema_version: 4,
    generated_by: "rlogs-bpsr-state-scaling-healing-proof",
    game_build: "1",
    source_rlogs: ["a.rlog"],
    inputs: [{ path: "a.rlog", bytes: 1, sha256: `sha256:${"a".repeat(64)}`, game_build: "1" }],
    policy: {
      exact_input_build_is_authoritative: true,
      exact_input_hashes_are_embedded: true,
      healing_events_are_discarded: false,
      unresolved_hp_formulas_are_hidden: false,
    },
    formula_families: [
      healingFixture(2406, 8, 200),
      healingFixture(21427, 1, 105),
    ],
  };
  const staticRows = [
    staticFixture(124060108, 2406, 8, "SpHeal"),
    staticFixture(22142701, 21427, 1, "SpHeal"),
    staticFixture(2220624103, 2206241, 3, "SpHeal", [2000]),
    staticFixture(2220624105, 2206241, 5, "Attack", [2000]),
  ];
  const exactEffectOccurrenceProof = {
    ...input,
    schema_version: 5,
    selection: { all_abilities: false, ability_ids: [2206241] },
    summary: {
      healing_events_scanned: 42,
      healing_events_selected: 0,
      selected_ability_ids: 0,
      selected_formula_families: 0,
      selected_amount_sum: 0,
    },
    formula_families: [],
  };
  const report = analyzeOperator(input, staticRows, {
    effectId: 2206241,
    healingProofInput: { path: "proof.json", bytes: 1, sha256: "0".repeat(64) },
    exactEffectOccurrenceProof,
    exactEffectOccurrenceProofInput: {
      path: "occurrence.json",
      bytes: 1,
      sha256: "3".repeat(64),
    },
    damageAttrInput: { path: "table.json", bytes: 1, sha256: "1".repeat(64) },
    formulaSurfaceInput: { path: "surface.json", bytes: 1, sha256: "2".repeat(64) },
  });
  if (report.summary.packet_observed_spheal_formula_families !== 2 ||
    report.summary.distinct_full_coverage_hp_basis_points.join(",") !== "105,200" ||
    report.summary.common_full_coverage_hp_basis_points_across_every_family.length !== 0 ||
    report.summary.damage_script_identity_alone_proves_operator !== false ||
    report.summary.exact_effect_output_packet_observed !== false ||
    report.summary.exact_effect_occurrence_proof_rlogs !== 1 ||
    report.summary.exact_effect_occurrence_proof_healing_events_scanned !== 42 ||
    report.interpretation.heterogeneous_spheal_family_evidence !== true ||
    report.interpretation.family_name_transfer_to_exact_effect_allowed !== false) {
    throw new Error("SpHeal operator proof self-test failed");
  }
  console.log("bpsr-spheal-operator-proof self-test passed");
}

function healingFixture(abilityId, hitEventId, basisPoints) {
  return {
    family: {
      ability_id: abilityId,
      hit_event_id: hitEventId,
      raw_attacker_uuid: 10,
    },
    events: 2,
    amount_min: 1,
    amount_max: 2,
    self_target_events: 2,
    source_entities: [10],
    target_entities: [10],
    reported_amount_candidates: [{
      basis: "wire_start_target_max_hp",
      candidates: [{
        basis_points: basisPoints,
        coverage_basis_points: 10_000,
        events: 2,
        distinct_numerators: 2,
        distinct_denominators: 2,
      }],
    }],
  };
}

function staticFixture(id, typeEnum, hitEventId, damageScript, coefficient = []) {
  return {
    damage_attr_id: id,
    type_enum: typeEnum,
    hit_event_suffix_candidate: hitEventId,
    damage_script: damageScript,
    coefficient_basis_points_by_stage: coefficient,
    fixed_parameter_by_level: [],
    pve_loop_time: 0,
    damage_type: 2,
    row_level: 0,
  };
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-spheal-operator-proof.mjs analyze --healing-proof <json> --exact-effect-occurrence-proof <json> --damage-attr-table <DamageAttrTable.json> --damage-formula-surface <semantic-surface.json> --effect <exact-id> --output <json>\n  node tools/bpsr-spheal-operator-proof.mjs self-test");
  process.exit(exitCode);
}
