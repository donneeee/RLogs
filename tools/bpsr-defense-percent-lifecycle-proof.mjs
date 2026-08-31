#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR = "tools/bpsr-defense-percent-lifecycle-proof.mjs";
const SCALE = 10_000n;
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "analyze") analyze(options);
else if (command === "verify") verifyFile(resolvePath(required(options, "input")), true);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(args) {
  const build = required(args, "build");
  const effectId = integer(required(args, "effect"), "effect");
  const attributeId = integer(required(args, "attribute"), "attribute");
  const files = {
    exact_wire_proof: resolvePath(required(args, "exact-wire-proof")),
    percent_family_proof: resolvePath(required(args, "percent-family-proof")),
    target_mitigation_rollup: resolvePath(required(args, "target-mitigation-rollup")),
    preflight: resolvePath(required(args, "preflight")),
    buff_table: resolvePath(required(args, "buff-table")),
  };
  const output = resolvePath(required(args, "output"));
  const proof = buildProof({ build, effectId, attributeId, files });
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(proof, null, 2)}\n`, "utf8");
  verifyFile(output, true);
  console.log(
    `Defense lifecycle proof for effect ${effectId}: ${proof.formula.percent_basis_points} bp; `
      + `${proof.summary.exact_wire_occurrences} exact transitions across `
      + `${proof.summary.independent_sessions} sessions; downstream damage credit=false.`,
  );
}

function buildProof({ build, effectId, attributeId, files }) {
  assert(/^\d+$/.test(build), "build must contain only ASCII digits");
  for (const [label, file] of Object.entries(files)) requireFile(file, label.replaceAll("_", " "));
  const wire = readJson(files.exact_wire_proof, "exact-wire proof");
  const percentFamilyProof = readJson(files.percent_family_proof, "percent-family proof");
  const rollup = readJson(files.target_mitigation_rollup, "target-mitigation rollup");
  const preflight = readJson(files.preflight, "build preflight");
  const buffTable = readJson(files.buff_table, "BuffTable");

  assert(
    wire.schema_version === 26 && wire.generated_by === "rlogs-bpsr-rdps-status-attribute-proof",
    "unsupported exact-wire status/attribute proof",
  );
  assert(wire.policy?.formula_inference === false, "source exact-wire proof changed its evidence policy");
  assert(wire.policy?.unresolved_evidence_is_hidden === false, "source exact-wire proof hides evidence");
  assert(
    wire.policy?.wire_message_state?.includes("same_capture_connection_and_stream"),
    "source exact-wire proof lacks exact wire-message identity",
  );
  assert((wire.selected_effect_ids ?? []).map(Number).includes(effectId), "effect is absent from exact-wire proof");
  assert((wire.reported_effect_ids ?? []).map(Number).includes(effectId), "effect is not reported by exact-wire proof");
  assert((wire.selected_attribute_ids ?? []).map(Number).includes(attributeId), "attribute is absent from exact-wire proof");
  assert(
    percentFamilyProof.schema_version === 26
      && percentFamilyProof.generated_by === "rlogs-bpsr-rdps-status-attribute-proof",
    "unsupported percent-family status/attribute proof",
  );
  assert(percentFamilyProof.policy?.formula_inference === false, "source percent-family proof changed its evidence policy");
  assert(percentFamilyProof.policy?.unresolved_evidence_is_hidden === false, "source percent-family proof hides evidence");
  assert(
    percentFamilyProof.policy?.wire_message_state?.includes("same_capture_connection_and_stream"),
    "source percent-family proof lacks exact wire-message identity",
  );
  assert((percentFamilyProof.selected_effect_ids ?? []).map(Number).includes(effectId), "effect is absent from percent-family proof");
  assert((percentFamilyProof.reported_effect_ids ?? []).map(Number).includes(effectId), "effect is not reported by percent-family proof");
  const requiredFamilyAttributeIds = [11350, 11351, 11352, 11353, 11354, 11355];
  const selectedFamilyAttributeIds = new Set((percentFamilyProof.selected_attribute_ids ?? []).map(Number));
  assert(
    requiredFamilyAttributeIds.every((id) => selectedFamilyAttributeIds.has(id)),
    "percent-family proof lacks the complete physical-defense attribute family",
  );
  assert(String(rollup.game_build ?? "") === build, "target-mitigation rollup build mismatch");
  assert(String(preflight.game_build ?? "") === build, "preflight build mismatch");
  assert(preflight.ready_for_snapshot === false, "current invocation expects the known blocked preflight");
  assert(preflight.runtime_promotion_allowed === false, "current invocation cannot use a promoted runtime preflight");

  const rollupRlogs = new Set(
    (rollup.runs ?? []).flatMap((run) => run.cohort?.source_inputs ?? []).map(normalizePath),
  );
  const wireRlogs = (wire.sessions ?? []).map((session) => normalizePath(session.rlog));
  assert(wireRlogs.length > 0, "exact-wire proof has no sessions");
  const unboundRlogs = wireRlogs.filter((rlog) => !rollupRlogs.has(rlog));
  assert(unboundRlogs.length === 0, `exact-wire sessions are absent from build-locked rollup: ${unboundRlogs.join(", ")}`);
  const percentFamilyRlogs = (percentFamilyProof.sessions ?? []).map((session) => normalizePath(session.rlog));
  assert(percentFamilyRlogs.length > 0, "percent-family proof has no sessions");
  const unboundPercentFamilyRlogs = percentFamilyRlogs.filter((rlog) => !rollupRlogs.has(rlog));
  assert(
    unboundPercentFamilyRlogs.length === 0,
    `percent-family sessions are absent from build-locked rollup: ${unboundPercentFamilyRlogs.join(", ")}`,
  );
  assert(
    wireRlogs.length === percentFamilyRlogs.length
      && wireRlogs.every((rlog) => percentFamilyRlogs.includes(rlog)),
    "exact-wire and percent-family proofs do not cover the same sessions",
  );

  const buff = buffTable[String(effectId)];
  assert(buff && Number(buff.Id) === effectId, `BuffTable lacks exact effect ${effectId}`);
  const equationSystem = (wire.wire_additive_equation_systems ?? [])
    .find((system) => Number(system.attribute_id) === attributeId);
  assert(equationSystem, `exact-wire proof lacks attribute equation system ${attributeId}`);
  const equations = (equationSystem.equations ?? []).filter((equation) =>
    equation.terms?.length === 1 && Number(equation.terms[0].effect_id) === effectId,
  );
  assert(equations.length > 0, "no single-effect exact-wire equations were found");

  let interval = { minimum: null, maximum: null };
  const observations = [];
  const sessionIds = new Set();
  const sourceEntities = new Set();
  const targetEntities = new Set();
  let applicationOccurrences = 0;
  let removalOccurrences = 0;
  for (const equation of equations) {
    const term = equation.terms[0];
    const sign = Number(term.signed_presence_delta);
    assert(sign === 1 || sign === -1, "defense equation has a non-binary presence transition");
    const examples = Array.isArray(equation.examples) ? equation.examples : [equation.examples].filter(Boolean);
    assert(
      examples.length === Number(equation.count),
      "exact-wire equation examples are truncated; rerun with a sufficient --example-limit",
    );
    for (const example of examples) {
      const before = exactBigInt(example.before_value, "before_value");
      const after = exactBigInt(example.after_value, "after_value");
      const base = sign === 1 ? before : after;
      const boosted = sign === 1 ? after : before;
      assert(base > 0n && boosted > base, "defense transition is not a positive multiplicative buff");
      assert(after - before === exactBigInt(equation.raw_attribute_delta, "raw_attribute_delta"), "wire delta does not conserve");
      const witnessInterval = basisPointInterval(base, boosted);
      interval = intersectIntervals(interval, witnessInterval);
      assert(interval.minimum <= interval.maximum, "exact defense transitions have no common integer basis-point formula");
      const status = (example.status_instances ?? []).find((row) => Number(row.effect_id) === effectId);
      assert(status, "exact-wire example lost its status instance");
      const targetEntity = exactBigInt(example.target_entity_uuid, "target_entity_uuid").toString();
      const sourceEntity = exactBigInt(status.source_entity_uuid, "source_entity_uuid").toString();
      assert(sourceEntity !== targetEntity, "defense proof requires an externally provided status witness");
      sessionIds.add(String(example.session_id));
      sourceEntities.add(sourceEntity);
      targetEntities.add(targetEntity);
      if (sign === 1) applicationOccurrences += 1;
      else removalOccurrences += 1;
      observations.push({
        session_id: String(example.session_id),
        wire_capture_sequence: Number(example.wire_capture_sequence),
        state: sign === 1 ? "applied" : "removed",
        wire_before_physical_defense: Number(before),
        wire_after_physical_defense: Number(after),
        source_entity_uuid: sourceEntity,
        target_entity_uuid: targetEntity,
        base_physical_defense: Number(base),
        buffed_physical_defense: Number(boosted),
        observed_delta: Number(boosted - base),
        compatible_basis_points: {
          minimum: Number(witnessInterval.minimum),
          maximum: Number(witnessInterval.maximum),
        },
      });
    }
  }
  assert(interval.minimum === interval.maximum, "exact transitions do not isolate one integer basis-point value");
  const basisPoints = interval.minimum;
  assert(applicationOccurrences > 0 && removalOccurrences > 0, "proof requires both application and removal witnesses");
  assert(sessionIds.size >= 2, "proof requires at least two independent sessions");
  for (const row of observations) {
    const predicted = scaled(BigInt(row.base_physical_defense), basisPoints);
    assert(predicted === BigInt(row.buffed_physical_defense), "isolated basis-point formula does not replay an exact witness");
  }

  const effectFamily = (percentFamilyProof.effects ?? [])
    .find((effect) => Number(effect.effect_id) === effectId);
  assert(effectFamily, "percent-family proof lacks the selected effect report");
  const physicalDefenseFamily = (effectFamily.percent_family_formulas ?? [])
    .find((family) => family.family === "physical_defense");
  assert(physicalDefenseFamily, "percent-family proof lacks physical_defense formula evidence");
  assert(Number(physicalDefenseFamily.final_attribute_id) === 11350, "physical-defense final attribute changed");
  assert(Number(physicalDefenseFamily.intermediate_attribute_id) === 11351, "physical-defense intermediate attribute changed");
  assert(Number(physicalDefenseFamily.base_attribute_id) === 11352, "physical-defense base attribute changed");
  assert(Number(physicalDefenseFamily.raw_extra_add_attribute_id) === 11353, "physical-defense raw extra-add attribute changed");
  assert(Number(physicalDefenseFamily.raw_percent_attribute_id) === 11354, "physical-defense raw-percent attribute changed");
  assert(Number(physicalDefenseFamily.raw_extra_percent_attribute_id) === 11355, "physical-defense raw extra-percent attribute changed");
  assert(Number(physicalDefenseFamily.scale) === Number(SCALE), "physical-defense percent scale changed");
  const exactFamilyInputs = Number(physicalDefenseFamily.transitions_with_exact_wire_inputs ?? 0);
  assert(exactFamilyInputs > 0, "percent-family proof has no exact wire inputs");
  assert(
    Number(physicalDefenseFamily.intermediate_exact_delta_matches) === exactFamilyInputs
      && Number(physicalDefenseFamily.intermediate_residual_mismatches) === 0,
    "integer-truncating intermediate formula does not replay all exact family inputs",
  );
  assert(
    Number(physicalDefenseFamily.nearest_intermediate_residual_mismatches) > 0,
    "percent-family proof does not distinguish truncation from nearest rounding",
  );
  assert(
    Number(physicalDefenseFamily.final_transitions_with_unknown_extra_percent) === exactFamilyInputs,
    "unknown raw extra-percent evidence was not preserved",
  );
  const familyExamples = [];
  for (const aggregate of physicalDefenseFamily.aggregates ?? []) {
    const examples = Array.isArray(aggregate.examples) ? aggregate.examples : [aggregate.examples].filter(Boolean);
    assert(examples.length === Number(aggregate.count), "percent-family examples are truncated; rerun with a sufficient --example-limit");
    for (const example of examples) familyExamples.push({ aggregate, example });
  }
  assert(familyExamples.length === exactFamilyInputs, "percent-family example conservation failed");

  const joinedRawPercentWitnesses = [];
  const unresolvedFinalOnlyWitnesses = [];
  for (const observation of observations) {
    const sign = observation.state === "applied" ? 1 : -1;
    const matches = familyExamples.filter(({ aggregate, example }) =>
      String(example.session_id) === observation.session_id
        && Number(example.wire_capture_sequence) === observation.wire_capture_sequence
        && String(example.state) === observation.state
        && Number(example.before_final_value) === observation.wire_before_physical_defense
        && Number(example.after_final_value) === observation.wire_after_physical_defense
        && Number(aggregate.raw_percent_delta_units) === sign * Number(basisPoints)
        && Number(aggregate.base_delta_units) === 0
        && Number(example.before_raw_percent) === (sign === 1 ? 0 : Number(basisPoints))
        && Number(example.after_raw_percent) === (sign === 1 ? Number(basisPoints) : 0)
        && Number(example.intermediate_residual_units) === 0
    );
    assert(matches.length <= 1, "a lifecycle transition has ambiguous raw-percent family witnesses");
    if (matches.length === 1) {
      joinedRawPercentWitnesses.push({
        session_id: observation.session_id,
        wire_capture_sequence: observation.wire_capture_sequence,
        state: observation.state,
        raw_percent_delta_basis_points: sign * Number(basisPoints),
        base_attribute_delta: 0,
        intermediate_formula_residual: 0,
      });
    } else {
      unresolvedFinalOnlyWitnesses.push({
        session_id: observation.session_id,
        wire_capture_sequence: observation.wire_capture_sequence,
        state: observation.state,
        wire_before_physical_defense: observation.wire_before_physical_defense,
        wire_after_physical_defense: observation.wire_after_physical_defense,
        reason: "raw physical-defense family fields were not packet-observed for this exact transition",
      });
    }
  }
  assert(joinedRawPercentWitnesses.length > 0, "no isolated lifecycle transition joins to a raw-percent packet witness");

  const grouped = new Map();
  for (const row of observations) {
    const key = `${row.state}|${row.base_physical_defense}|${row.buffed_physical_defense}`;
    const value = grouped.get(key) ?? {
      state: row.state,
      base_physical_defense: row.base_physical_defense,
      buffed_physical_defense: row.buffed_physical_defense,
      observed_delta: row.observed_delta,
      occurrences: 0,
      sessions: new Set(),
    };
    value.occurrences += 1;
    value.sessions.add(row.session_id);
    grouped.set(key, value);
  }
  const exactTransitions = [...grouped.values()]
    .map((row) => ({ ...row, sessions: [...row.sessions].sort() }))
    .sort((left, right) => left.state.localeCompare(right.state)
      || left.base_physical_defense - right.base_physical_defense);

  const report = {
    schema_version: 2,
    generated_by: GENERATOR,
    generated_at: new Date().toISOString(),
    game_build: build,
    effect_id: effectId,
    attribute_id: attributeId,
    policy: {
      exact_numeric_effect_attribute_and_build_identity_are_authoritative: true,
      localized_names_are_evidence_only: true,
      exact_wire_single_effect_equations_required: true,
      structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
      local_recipient_attribute_observations_are_sufficient_for_this_stat_formula: true,
      final_only_observations_remain_visible_and_do_not_imply_raw_field_observation: true,
      stat_formula_proof_does_not_grant_damage_formula_authority: true,
      unresolved_downstream_damage_evidence_is_hidden: false,
      ordinary_damage_is_not_modified_by_this_proof: true,
    },
    inputs: Object.fromEntries(Object.entries(files).map(([key, file]) => [key, fileIdentity(file)])),
    build_identity: {
      source_rollup_game_build: String(rollup.game_build),
      exact_wire_sessions: wireRlogs.length,
      exact_wire_sessions_bound_to_build_rollup: wireRlogs.length,
      percent_family_sessions: percentFamilyRlogs.length,
      percent_family_sessions_bound_to_build_rollup: percentFamilyRlogs.length,
      preflight_ready_for_snapshot: preflight.ready_for_snapshot === true,
      runtime_promotion_allowed: preflight.runtime_promotion_allowed === true,
    },
    exact_effect_identity: {
      buff_table_id: Number(buff.Id),
      buff_table_level_field: Number(buff.Level),
      observed_packet_level: Number(equations[0].terms[0].level),
      semantic_name_design_evidence_only: buff.NameDesign ?? null,
      origin_source_type_id: Number(equations[0].terms[0].origin?.source_type_id),
      origin_source_config_id: Number(equations[0].terms[0].origin?.source_config_id),
    },
    formula: {
      operation: "integer-truncating multiplicative percent increase",
      expression: "buffed_physical_defense = trunc(base_physical_defense * (10000 + percent_basis_points) / 10000)",
      scale: Number(SCALE),
      percent_basis_points: Number(basisPoints),
      percent: Number(basisPoints) / 100,
      percent_basis_points_source: "packet-observed raw-percent attribute 11354 where present, plus exact final-stat lifecycle replay",
      compatible_integer_basis_point_interval: {
        minimum: Number(interval.minimum),
        maximum: Number(interval.maximum),
      },
      multiplication_before_integer_truncation_proven: true,
      application_and_removal_replay_exact: true,
    },
    packet_raw_percent_proof: {
      family: "physical_defense",
      final_attribute_id: 11350,
      intermediate_attribute_id: 11351,
      base_attribute_id: 11352,
      raw_extra_add_attribute_id: 11353,
      raw_percent_attribute_id: 11354,
      raw_extra_percent_attribute_id: 11355,
      scale: Number(SCALE),
      exact_family_input_transitions: exactFamilyInputs,
      exact_intermediate_formula_matches: Number(physicalDefenseFamily.intermediate_exact_delta_matches),
      intermediate_formula_residual_mismatches: Number(physicalDefenseFamily.intermediate_residual_mismatches),
      nearest_rounding_residual_mismatches: Number(physicalDefenseFamily.nearest_intermediate_residual_mismatches),
      truncation_selected_over_round_to_nearest: true,
      joined_exact_single_effect_occurrences: joinedRawPercentWitnesses.length,
      unresolved_final_only_occurrences: unresolvedFinalOnlyWitnesses.length,
      all_joined_raw_percent_deltas_equal_effect_basis_points: true,
      raw_percent_identity_for_all_lifecycle_occurrences_proven: unresolvedFinalOnlyWitnesses.length === 0,
      raw_extra_percent_packet_known_for_exact_family_inputs: false,
      joined_witnesses: joinedRawPercentWitnesses,
      unresolved_final_only_witnesses: unresolvedFinalOnlyWitnesses,
    },
    exact_transition_aggregates: exactTransitions,
    summary: {
      exact_single_effect_equation_signatures: equations.length,
      exact_wire_occurrences: observations.length,
      packet_raw_percent_joined_occurrences: joinedRawPercentWitnesses.length,
      final_only_unresolved_occurrences: unresolvedFinalOnlyWitnesses.length,
      application_occurrences: applicationOccurrences,
      removal_occurrences: removalOccurrences,
      independent_sessions: sessionIds.size,
      distinct_external_sources: sourceEntities.size,
      recipient_entities: targetEntities.size,
      distinct_base_values: new Set(observations.map((row) => row.base_physical_defense)).size,
      exact_defense_stat_formula_proven: true,
      exact_target_defense_to_damage_formula_proven: false,
      provider_rdps_credit_allowed: false,
      ui_rdps_display_authority: false,
    },
    downstream_damage_gate: {
      status: "defense-stat-formula-proven-damage-counterfactual-unproven",
      exact_defense_stat_formula_proven: true,
      exact_target_defense_to_damage_formula_proven: false,
      exact_damage_operation_order_proven: false,
      exact_damage_integer_rounding_proven: false,
      packet_damage_conservation_proven: false,
      hidden_additional_damage_stage_behavior_excluded: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      blockers: [
        "a controlled locally observable defense-to-damage counterfactual is still required",
        "the exact target mitigation curve and damage-stage integer rounding remain unproven",
        "the stat transition alone cannot exclude an additional hidden damage-stage effect",
      ],
    },
  };
  report.content_sha256 = hashJson(report);
  verifyProof(report, false);
  return report;
}

function verifyFile(file, verifyInputs) {
  const proof = readJson(file, "defense percent lifecycle proof");
  verifyProof(proof, verifyInputs);
  console.log(
    `Verified effect ${proof.effect_id} defense formula: ${proof.formula.percent_basis_points} bp; `
      + `damage credit allowed=${proof.summary.provider_rdps_credit_allowed}.`,
  );
}

function verifyProof(proof, verifyInputs) {
  assert([1, 2].includes(proof.schema_version) && proof.generated_by === GENERATOR, "unsupported defense proof schema or generator");
  assert(/^\d+$/.test(String(proof.game_build ?? "")), "defense proof lacks a valid build");
  assert(proof.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements === true, "remote packet policy changed");
  assert(proof.policy?.ordinary_damage_is_not_modified_by_this_proof === true, "ordinary damage preservation policy changed");
  assert(proof.summary?.exact_defense_stat_formula_proven === true, "defense stat formula is not proven");
  assert(proof.formula?.compatible_integer_basis_point_interval?.minimum === proof.formula?.percent_basis_points, "basis-point lower bound is not exact");
  assert(proof.formula?.compatible_integer_basis_point_interval?.maximum === proof.formula?.percent_basis_points, "basis-point upper bound is not exact");
  assert(proof.summary?.application_occurrences > 0 && proof.summary?.removal_occurrences > 0, "proof lacks bidirectional lifecycle evidence");
  assert(proof.summary?.independent_sessions >= 2, "proof lacks independent sessions");
  assert(proof.summary?.provider_rdps_credit_allowed === false, "stat proof improperly grants rDPS credit");
  assert(proof.downstream_damage_gate?.runtime_authority === false, "stat proof improperly grants runtime authority");
  assert(proof.downstream_damage_gate?.provider_rdps_credit_allowed === false, "downstream gate improperly grants credit");
  assert(proof.downstream_damage_gate?.blockers?.length > 0, "downstream damage blockers were lost");
  const aggregateOccurrences = (proof.exact_transition_aggregates ?? [])
    .reduce((sum, row) => sum + Number(row.occurrences ?? 0), 0);
  assert(aggregateOccurrences === proof.summary.exact_wire_occurrences, "transition occurrence conservation failed");
  if (proof.schema_version === 2) {
    const raw = proof.packet_raw_percent_proof;
    assert(raw?.family === "physical_defense", "schema 2 proof lacks the physical-defense family");
    assert(raw.final_attribute_id === 11350 && raw.intermediate_attribute_id === 11351, "schema 2 final/intermediate attributes changed");
    assert(raw.base_attribute_id === 11352 && raw.raw_extra_add_attribute_id === 11353, "schema 2 base/add attributes changed");
    assert(raw.raw_percent_attribute_id === 11354 && raw.raw_extra_percent_attribute_id === 11355, "schema 2 percent attributes changed");
    assert(raw.scale === Number(SCALE), "schema 2 percent scale changed");
    assert(raw.exact_family_input_transitions > 0, "schema 2 proof lacks exact family inputs");
    assert(raw.exact_intermediate_formula_matches === raw.exact_family_input_transitions, "schema 2 exact family replay is incomplete");
    assert(raw.intermediate_formula_residual_mismatches === 0, "schema 2 exact family replay has residuals");
    assert(raw.nearest_rounding_residual_mismatches > 0, "schema 2 proof does not reject nearest rounding");
    assert(raw.truncation_selected_over_round_to_nearest === true, "schema 2 truncation selection was lost");
    assert(raw.joined_exact_single_effect_occurrences > 0, "schema 2 proof lacks joined raw-percent witnesses");
    assert(
      raw.joined_exact_single_effect_occurrences + raw.unresolved_final_only_occurrences
        === proof.summary.exact_wire_occurrences,
      "schema 2 lifecycle/raw-percent join conservation failed",
    );
    assert(
      raw.joined_witnesses?.length === raw.joined_exact_single_effect_occurrences,
      "schema 2 joined witness count changed",
    );
    assert(
      raw.unresolved_final_only_witnesses?.length === raw.unresolved_final_only_occurrences,
      "schema 2 unresolved final-only witness count changed",
    );
    assert(raw.all_joined_raw_percent_deltas_equal_effect_basis_points === true, "schema 2 joined basis points diverged");
    assert(raw.raw_extra_percent_packet_known_for_exact_family_inputs === false, "schema 2 invented raw extra-percent knowledge");
    assert(
      proof.summary.packet_raw_percent_joined_occurrences === raw.joined_exact_single_effect_occurrences
        && proof.summary.final_only_unresolved_occurrences === raw.unresolved_final_only_occurrences,
      "schema 2 summary/raw-percent counts diverged",
    );
  }
  const { content_sha256: recordedHash, ...withoutHash } = proof;
  assert(recordedHash === hashJson(withoutHash), "defense proof content hash is invalid");
  if (verifyInputs) {
    for (const [label, input] of Object.entries(proof.inputs ?? {})) {
      const file = resolvePath(input.path);
      requireFile(file, label.replaceAll("_", " "));
      assert(statSync(file).size === input.bytes, `${label} byte length changed`);
      assert(sha256(file) === input.sha256, `${label} content hash changed`);
    }
  }
}

function basisPointInterval(base, boosted) {
  const minimum = ceilDiv(boosted * SCALE, base) - SCALE;
  const maximum = ceilDiv((boosted + 1n) * SCALE, base) - 1n - SCALE;
  return { minimum, maximum };
}

function intersectIntervals(left, right) {
  return {
    minimum: left.minimum === null || right.minimum > left.minimum ? right.minimum : left.minimum,
    maximum: left.maximum === null || right.maximum < left.maximum ? right.maximum : left.maximum,
  };
}

function scaled(base, basisPoints) {
  return base * (SCALE + basisPoints) / SCALE;
}

function ceilDiv(numerator, denominator) {
  assert(denominator > 0n && numerator >= 0n, "ceilDiv requires nonnegative numerator and positive denominator");
  return (numerator + denominator - 1n) / denominator;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-defense-percent-proof-"));
  try {
    const files = {
      exact_wire_proof: path.join(root, "wire.json"),
      percent_family_proof: path.join(root, "percent-family.json"),
      target_mitigation_rollup: path.join(root, "rollup.json"),
      preflight: path.join(root, "preflight.json"),
      buff_table: path.join(root, "buff.json"),
    };
    const term = { effect_id: 2201452, origin: { source_type_id: 1, source_config_id: 50049 }, level: 30, stacks: 1, count: -1 };
    const example = (session, state, before, after, sequence) => ({
      session_id: session,
      wire_capture_sequence: sequence,
      target_entity_uuid: 20,
      before_value: before,
      after_value: after,
      status_instances: [{ effect_id: 2201452, state, source_entity_uuid: 10 }],
    });
    writeJson(files.exact_wire_proof, {
      schema_version: 26,
      generated_by: "rlogs-bpsr-rdps-status-attribute-proof",
      policy: {
        formula_inference: false,
        unresolved_evidence_is_hidden: false,
        wire_message_state: "same_capture_connection_and_stream_are_one_message",
      },
      selected_effect_ids: [2201452],
      reported_effect_ids: [2201452],
      selected_attribute_ids: [11350],
      sessions: [{ rlog: "runtime-data/logs/a.rlog" }, { rlog: "runtime-data/logs/b.rlog" }],
      wire_additive_equation_systems: [{
        attribute_id: 11350,
        equations: [
          { terms: [{ ...term, signed_presence_delta: 1 }], raw_attribute_delta: 537, count: 2, examples: [
            example("a", "applied", 5370, 5907, 1), example("b", "applied", 5370, 5907, 2),
          ] },
          { terms: [{ ...term, signed_presence_delta: -1 }], raw_attribute_delta: -581, count: 2, examples: [
            example("a", "removed", 6399, 5818, 3), example("b", "removed", 6399, 5818, 4),
          ] },
        ],
      }],
    });
    const familyExample = (session, state, beforeFinal, afterFinal, beforeBase, beforePercent, afterPercent, sequence) => ({
      session_id: session,
      wire_capture_sequence: sequence,
      state,
      before_final_value: beforeFinal,
      after_final_value: afterFinal,
      before_intermediate_value: beforeFinal,
      after_intermediate_value: afterFinal,
      before_base_add: beforeBase,
      after_base_add: beforeBase,
      before_raw_percent: beforePercent,
      after_raw_percent: afterPercent,
      intermediate_residual_units: 0,
    });
    const familyAggregate = (state, rawPercentDelta, intermediateDelta, examples) => ({
      state,
      raw_percent_delta_units: rawPercentDelta,
      base_delta_units: 0,
      intermediate_delta_units: intermediateDelta,
      predicted_intermediate_delta_units: intermediateDelta,
      intermediate_residual_units: 0,
      count: examples.length,
      examples,
    });
    writeJson(files.percent_family_proof, {
      schema_version: 26,
      generated_by: "rlogs-bpsr-rdps-status-attribute-proof",
      policy: {
        formula_inference: false,
        unresolved_evidence_is_hidden: false,
        wire_message_state: "same_capture_connection_and_stream_are_one_message",
      },
      selected_effect_ids: [2201452],
      reported_effect_ids: [2201452],
      selected_attribute_ids: [11350, 11351, 11352, 11353, 11354, 11355],
      sessions: [{ rlog: "runtime-data/logs/a.rlog" }, { rlog: "runtime-data/logs/b.rlog" }],
      effects: [{
        effect_id: 2201452,
        percent_family_formulas: [{
          family: "physical_defense",
          final_attribute_id: 11350,
          intermediate_attribute_id: 11351,
          base_attribute_id: 11352,
          raw_extra_add_attribute_id: 11353,
          raw_percent_attribute_id: 11354,
          raw_extra_percent_attribute_id: 11355,
          scale: 10000,
          transitions_with_exact_wire_inputs: 3,
          intermediate_exact_delta_matches: 3,
          intermediate_residual_mismatches: 0,
          nearest_intermediate_residual_mismatches: 2,
          final_transitions_with_unknown_extra_percent: 3,
          aggregates: [
            familyAggregate("applied", 1000, 537, [
              familyExample("a", "applied", 5370, 5907, 5370, 0, 1000, 1),
            ]),
            familyAggregate("removed", -1000, -581, [
              familyExample("a", "removed", 6399, 5818, 5818, 1000, 0, 3),
              familyExample("b", "removed", 6399, 5818, 5818, 1000, 0, 4),
            ]),
          ],
        }],
      }],
    });
    writeJson(files.target_mitigation_rollup, {
      game_build: "123",
      runs: [{ cohort: { source_inputs: ["runtime-data/logs/a.rlog", "runtime-data/logs/b.rlog"] } }],
    });
    writeJson(files.preflight, { game_build: "123", ready_for_snapshot: false, runtime_promotion_allowed: false });
    writeJson(files.buff_table, { "2201452": { Id: 2201452, Level: 52, NameDesign: "evidence only" } });
    const proof = buildProof({ build: "123", effectId: 2201452, attributeId: 11350, files });
    assert(proof.formula.percent_basis_points === 1000, "self-test did not isolate 1000 basis points");
    assert(proof.summary.exact_wire_occurrences === 4, "self-test occurrence conservation failed");
    assert(proof.schema_version === 2, "self-test did not produce schema 2");
    assert(proof.packet_raw_percent_proof.joined_exact_single_effect_occurrences === 3, "self-test raw-percent join failed");
    assert(proof.packet_raw_percent_proof.unresolved_final_only_occurrences === 1, "self-test did not preserve final-only evidence");
    console.log("bpsr-defense-percent-lifecycle-proof self-test passed.");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`unexpected argument: ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function required(args, key) {
  if (!args[key]) throw new Error(`missing required --${key}`);
  return args[key];
}

function integer(value, label) {
  if (!/^\d+$/.test(String(value))) throw new Error(`${label} must contain only ASCII digits`);
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) throw new Error(`${label} is not a safe integer`);
  return parsed;
}

function exactBigInt(value, label) {
  try {
    return BigInt(value);
  } catch {
    throw new Error(`${label} is not an exact integer`);
  }
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function normalizePath(value) {
  return String(value).replaceAll("\\", "/");
}

function relativePath(file) {
  return normalizePath(path.relative(repoRoot, file));
}

function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`missing ${label}: ${file}`);
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`invalid ${label} JSON at ${file}: ${error.message}`);
  }
}

function writeJson(file, value) {
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function fileIdentity(file) {
  return { path: relativePath(file), bytes: statSync(file).size, sha256: sha256(file) };
}

function sha256(file) {
  return `sha256:${createHash("sha256").update(readFileSync(file)).digest("hex")}`;
}

function hashJson(value) {
  return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-defense-percent-lifecycle-proof.mjs analyze --build <id> \\
    --effect <id> --attribute <id> --exact-wire-proof <json> \\
    --percent-family-proof <json> \\
    --target-mitigation-rollup <json> --preflight <json> --buff-table <json> \\
    --output <json>
  node tools/bpsr-defense-percent-lifecycle-proof.mjs verify --input <json>
  node tools/bpsr-defense-percent-lifecycle-proof.mjs self-test`);
  process.exit(exitCode);
}
