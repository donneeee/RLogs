#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SCHEMA_VERSION = 4;
const GENERATOR = "tools/target-mitigation-offline-exhaustion-proof.mjs";
const SINGLE_PACKET_PROOF_GENERATOR = "rlogs-bpsr-target-mitigation-transform-proof";
const PACKET_ROLLUP_GENERATOR = "tools/bpsr-target-mitigation-proof-rollup.mjs";
const options = parseArgs(process.argv.slice(2));
const luaDefinitionAuditPath = resolvePath(options.luaDefinitionAudit);
const luaConsumerAuditPath = resolvePath(options.luaConsumerAudit);
const directCallsiteAuditPath = resolvePath(options.directCallsiteAudit);
const fightAttributeProofPath = resolvePath(options.fightAttributeProof);
const packetPairProofPath = resolvePath(options.packetPairProof);
const neutralActionIdentityPath = resolvePath(options.neutralActionIdentity);
const neutralMitigationDiagnosticPath = resolvePath(options.neutralMitigationDiagnostic);
const outputPath = resolvePath(options.output);

const luaDefinitionAudit = readJson(luaDefinitionAuditPath, "AttackSimply Lua definition audit");
const luaConsumerAudit = readJson(luaConsumerAuditPath, "AttackSimply Lua consumer audit");
const directCallsiteAudit = readJson(directCallsiteAuditPath, "AttackSimply native callsite audit");
const fightAttributeProof = readJson(fightAttributeProofPath, "fight-attribute evaluator proof");
const packetPairProof = readJson(packetPairProofPath, "packet target-mitigation pair proof");
const neutralActionIdentity = readJson(
  neutralActionIdentityPath,
  "neutral mitigation action identity proof",
);
const neutralMitigationDiagnostic = readJson(
  neutralMitigationDiagnosticPath,
  "neutral mitigation actor-scene diagnostic",
);

const expectedBuild = String(options.gameBuild);
const expectedPacketBuild = String(options.packetBuild);
if (String(directCallsiteAudit.game_build) !== expectedBuild) {
  throw new Error(`direct-call audit build ${directCallsiteAudit.game_build} differs from ${expectedBuild}`);
}
if (String(fightAttributeProof.game_build) !== expectedBuild) {
  throw new Error(`fight-attribute proof build ${fightAttributeProof.game_build} differs from ${expectedBuild}`);
}
if (String(packetPairProof.game_build) !== expectedPacketBuild) {
  throw new Error(`packet target-mitigation proof build ${packetPairProof.game_build} differs from ${expectedPacketBuild}`);
}
if (Number(luaDefinitionAudit.summary?.parse_failures || 0) !== 0
  || Number(luaConsumerAudit.summary?.parse_failures || 0) !== 0) {
  throw new Error("AttackSimply Lua audit has parse failures");
}
if (Number(luaConsumerAudit.summary?.files_scanned || 0) < 4000
  || Number(luaConsumerAudit.summary?.files_with_matches || 0) !== 1
  || Number(luaConsumerAudit.summary?.functions_with_matches || 0) !== 1
  || Number(luaDefinitionAudit.summary?.files_with_matches || 0) !== 1) {
  throw new Error("AttackSimply Lua definition is not isolated to exactly one generated-table function");
}
if (Number(directCallsiteAudit.summary?.selected_method_names || 0) !== 3
  || Number(directCallsiteAudit.summary?.unique_target_rvas || 0) !== 3
  || Number(directCallsiteAudit.summary?.direct_callsites || 0) !== 0) {
  throw new Error("AttackSimply direct-call inventory changed");
}
if (directCallsiteAudit.policy?.indirect_calls_are_not_claimed_absent !== true) {
  throw new Error("direct-call audit must retain indirect-call uncertainty");
}
if (fightAttributeProof.proof_state !== "exact-current-build-client-ui-evaluator"
  || fightAttributeProof.policy?.combat_damage_stage_authority !== false
  || fightAttributeProof.policy?.unresolved_evidence_is_hidden !== false
  || Number(fightAttributeProof.summary?.exact_consumers || 0) !== 2) {
  throw new Error("fight-attribute evaluator proof is not the exact fail-closed UI surface");
}
const packetEvidenceScope = validatePacketEvidence(packetPairProof, expectedPacketBuild);
const neutralActionEvidence = validateNeutralActionEvidence(
  neutralActionIdentity,
  neutralActionIdentityPath,
  neutralMitigationDiagnostic,
  expectedPacketBuild,
);

const instructions = luaDefinitionAudit.files?.[0]?.matches?.[0]?.instructions || [];
const simpleConstants = extractAttackSimplyConstants(instructions);
const packetAxes = Object.entries(packetPairProof.axes || {}).map(([axis, value]) => ({
  axis,
  current_attribute_id: Number(value.current_attribute_id),
  required_packet_property: value.required_packet_property ?? null,
  samples_with_axis: Number(value.counters?.samples_with_axis || 0),
  controlled_groups: Number(value.counters?.controlled_groups || 0),
  deterministic_pairs: Number(value.counters?.deterministic_pairs || 0),
  divergent_output_pairs: Number(value.counters?.divergent_output_pairs || 0),
}));
if (packetAxes.length !== 12) throw new Error(`expected 12 mitigation axes, found ${packetAxes.length}`);
if (packetAxes.some((row) => row.controlled_groups !== 0
  || row.deterministic_pairs !== 0
  || row.divergent_output_pairs !== 0)) {
  throw new Error("archived target-mitigation proof now has a controlled pair and must be re-evaluated");
}

const seasonThree = fightAttributeProof.rows?.find((row) => Number(row.season_id) === 3);
if (!seasonThree) throw new Error("fight-attribute evaluator proof lacks current season row 3");
const uiCandidates = Object.fromEntries(
  ["DefPara", "RefDefPara", "ElementDefToDamRes"].map((field) => {
    const value = seasonThree.fields?.[field];
    if (value?.state !== "exact-current-build-parameter-array" || value.parameters?.length !== 7) {
      throw new Error(`fight-attribute field ${field} is not an exact seven-parameter current-build row`);
    }
    return [field, {
      parameters: value.parameters,
      exact_expression: value.exact_expression,
      authority: "character-sheet-display-transform-only",
    }];
  }),
);

const result = {
  schema_version: SCHEMA_VERSION,
  generated_by: GENERATOR,
  game: "blue-protocol-star-resonance",
  game_build: expectedBuild,
  packet_build: expectedPacketBuild,
  proof_state: expectedBuild === expectedPacketBuild && packetEvidenceScope.artifact_kind === "matching-build-formula-cohort-rollup"
    ? "exact-current-build-aggregate-offline-client-and-packet-search-exhausted-final-validation-required"
    : expectedBuild === expectedPacketBuild
      ? "exact-current-build-offline-client-and-packet-search-exhausted-final-validation-required"
    : "offline-client-and-archive-exhausted-final-validation-required",
  policy: {
    exact_build_required: true,
    exact_input_hashes_are_embedded: true,
    unresolved_evidence_is_hidden: false,
    candidate_constants_are_combat_formula_authority: false,
    character_sheet_transform_is_combat_formula_authority: false,
    absence_of_direct_calls_proves_absence_of_indirect_consumers: false,
    no_formula_is_promoted_without_controlled_packet_counterfactuals: true,
    archived_zero_pair_result_is_not_formula_proof: true,
    matching_build_packet_validation_is_required: true,
    new_client_or_packet_evidence_reopens_offline_exhaustion: true,
    matching_build_formula_cohort_rollup_is_bound:
      packetEvidenceScope.artifact_kind === "matching-build-formula-cohort-rollup",
    rollup_proves_repository_wide_capture_completeness: false,
    mitigation_action_targets_are_allegiance_neutral: true,
    current_actor_snapshots_are_never_substituted: true,
    exact_client_combat_damage_consumer_proven: false,
    server_combat_implementation_is_available_in_client_files: false,
  },
  inputs: {
    lua_definition_audit: fileDescriptor(luaDefinitionAuditPath),
    lua_consumer_audit: fileDescriptor(luaConsumerAuditPath),
    direct_callsite_audit: fileDescriptor(directCallsiteAuditPath),
    fight_attribute_proof: fileDescriptor(fightAttributeProofPath),
    packet_pair_proof: fileDescriptor(packetPairProofPath),
    neutral_action_identity: fileDescriptor(neutralActionIdentityPath),
    neutral_mitigation_diagnostic: fileDescriptor(neutralMitigationDiagnosticPath),
  },
  summary: {
    offline_exhausted_model_ids: [
      "target-physical-armor-counterfactual",
      "elemental-resistance-counterfactual",
    ],
    final_validation_obligations: 2,
    lua_files_scanned: Number(luaConsumerAudit.summary.files_scanned),
    lua_files_with_attack_simply_names: Number(luaConsumerAudit.summary.files_with_matches),
    native_direct_callsites: Number(directCallsiteAudit.summary.direct_callsites),
    exact_character_sheet_consumers: Number(fightAttributeProof.summary.exact_consumers),
    packet_axes_audited: packetAxes.length,
    packet_capture_proofs: packetEvidenceScope.matching_build_capture_proofs,
    packet_source_rlogs: packetEvidenceScope.matching_build_source_rlogs,
    packet_damage_samples: packetEvidenceScope.damage_samples,
    packet_audited_axis_samples: packetEvidenceScope.audited_axis_samples,
    packet_maximum_measured_peak_working_set_bytes:
      packetEvidenceScope.maximum_measured_peak_working_set_bytes,
    packet_samples_with_physical_or_refined_defense: packetAxes
      .filter((row) => [11350, 11420].includes(row.current_attribute_id))
      .reduce((sum, row) => sum + row.samples_with_axis, 0),
    packet_samples_with_magic_defense: packetAxes
      .filter((row) => row.current_attribute_id === 11360)
      .reduce((sum, row) => sum + row.samples_with_axis, 0),
    packet_samples_with_elemental_defense: packetAxes
      .filter((row) => row.current_attribute_id >= 13200)
      .reduce((sum, row) => sum + row.samples_with_axis, 0),
    controlled_counterfactual_pairs: packetAxes.reduce((sum, row) => sum + row.deterministic_pairs, 0),
    neutral_mitigation_actions: neutralActionEvidence.actions,
    neutral_player_targets: neutralActionEvidence.player_targets,
    event_time_damage_actors: neutralActionEvidence.source_actors,
    event_time_run_scenes: neutralActionEvidence.scenes,
    actor_scene_controlled_axis_pairs: neutralActionEvidence.controlled_axis_pairs,
    promoted_combat_formulas: 0,
  },
  current_build_client_candidates: {
    attack_simply: simpleConstants,
    character_sheet_transforms: uiCandidates,
  },
  packet_evidence_scope: packetEvidenceScope,
  neutral_action_evidence: neutralActionEvidence,
  archived_packet_counterfactuals: packetAxes,
  final_validation: [
    {
      model_id: "target-physical-armor-counterfactual",
      requirement: "matching-build controlled target physical/refined defense pair with identical source state, calculation identity, target status state, and divergent deterministic output",
    },
    {
      model_id: "elemental-resistance-counterfactual",
      requirement: "matching-build controlled target elemental-defense pair with identical source state, calculation identity, target status state, packet property, and divergent deterministic output",
    },
  ],
};

result.content_sha256 = contentHash(result);
verifyResult(result);
writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
verifyResult(readJson(outputPath, "written target mitigation offline exhaustion proof"));
console.log(JSON.stringify(result.summary, null, 2));

function extractAttackSimplyConstants(rows) {
  const instructionAt = new Map(rows.map((row) => [Number(row.pc), row]));
  const scalar = extractAssignedValue("AttackSimplyDefParam");
  const refined = extractAssignedValue("AttackSimplyRefineDefParam");
  const levelStart = findLoad("AttackSimplyDeltaLevelMultiParam");
  const values = [];
  for (let pc = levelStart.pc + 2; pc < levelStart.pc + 40; pc += 1) {
    const row = instructionAt.get(pc);
    if (!row) continue;
    if (row.opcode === "SETLIST") break;
    if (row.opcode === "LOADK" && Number.isSafeInteger(Number(row.operands?.constant))) {
      values.push(Number(row.operands.constant));
    }
  }
  const expected = [1, 2, 2, 3, 4, 5, 7, 9, 11, 13, 16, 19, 22, 25, 30];
  if (scalar !== 6500 || refined !== 6500 || JSON.stringify(values) !== JSON.stringify(expected)) {
    throw new Error("AttackSimply exact current-build constants changed");
  }
  return {
    AttackSimplyDefParam: scalar,
    AttackSimplyRefineDefParam: refined,
    AttackSimplyDeltaLevelMultiParam: values,
    authority: "generated-client-definition-candidate-only",
  };

  function extractAssignedValue(name) {
    const load = findLoad(name);
    const value = instructionAt.get(load.pc + 1);
    const assignment = instructionAt.get(load.pc + 2);
    if (value?.opcode !== "LOADK" || assignment?.opcode !== "SETTABLE") {
      throw new Error(`could not prove exact generated assignment for ${name}`);
    }
    return Number(value.operands?.constant);
  }

  function findLoad(name) {
    const matches = rows.filter((row) => row.opcode === "LOADK" && row.operands?.constant === name);
    if (matches.length !== 1) throw new Error(`expected one generated assignment for ${name}`);
    return matches[0];
  }
}

function validateNeutralActionEvidence(identity, identityPath, diagnostic, build) {
  const identityDescriptor = fileDescriptor(identityPath);
  const observations = identity?.observations ?? [];
  const playerTargets = observations.filter((row) =>
    row.actor_active === true && row.actor_kind === "player"
  ).length;
  const sourceActors = observations.filter((row) => row.source_actor_active === true).length;
  const scenes = observations.filter((row) => row.scene_id != null).length;
  const classSpecialization117 = observations.filter((row) =>
    Number(row.class_id) === 11 && Number(row.specialization_id) === 117
  ).length;
  const classUnresolvedSpecialization = observations.filter((row) =>
    Number(row.class_id) === 11 && row.specialization_id == null
  ).length;
  if (Number(identity?.schema_version) !== 2 ||
    identity?.generated_by !== "rlogs-bpsr-selected-action-target-identity-proof" ||
    String(identity?.game_build) !== String(build) ||
    Number(identity?.summary?.requested_actions) !== 774 ||
    Number(identity?.summary?.matched_actions) !== 774 ||
    Number(identity?.summary?.missing_actions) !== 0 ||
    Number(identity?.summary?.observations_with_identity_conflict) !== 0 ||
    Number(identity?.summary?.observations_with_source_identity_conflict) !== 0 ||
    observations.length !== 774 || playerTargets !== 774 || sourceActors !== 773 ||
    scenes !== 764 || classSpecialization117 !== 754 ||
    classUnresolvedSpecialization !== 20 ||
    identity?.policy?.target_endpoint_is_allegiance_neutral !== true ||
    identity?.policy?.recipient_or_enemy_target_are_both_allowed !== true ||
    identity?.policy?.absent_monster_or_character_identity_zero_filled !== false ||
    identity?.policy?.static_target_stats_substituted !== false ||
    identity?.policy?.provider_rdps_credit_allowed !== false) {
    throw new Error("neutral mitigation action identity is unsafe or incomplete");
  }
  const enrichment = diagnostic?.target_identity_enrichment;
  const axes = [
    [diagnostic?.axes?.physical_defense, 765, 747],
    [diagnostic?.axes?.magic_defense, 765, 747],
    [diagnostic?.axes?.refined_defense, 764, 756],
  ];
  for (const [axis, samples, contexts] of axes) {
    if (Number(axis?.counters?.samples_with_axis) !== samples ||
      Number(axis?.counters?.samples_with_packet_observed_target_actor_identity) !== samples ||
      Number(axis?.counters?.samples_with_cross_capture_actor_shape_context) !== contexts ||
      Number(axis?.counters?.groups_with_multiple_axis_states) !== 0 ||
      Number(axis?.counters?.distinct_axis_pairs) !== 0 ||
      Number(axis?.counters?.pairs_with_cross_capture_witness) !== 0) {
      throw new Error("neutral mitigation axis evidence changed");
    }
  }
  if (Number(diagnostic?.schema_version) !== 3 ||
    diagnostic?.generated_by !==
      "rlogs-bpsr-target-mitigation-transform-proof:cross-capture-target-config-diagnostic" ||
    String(diagnostic?.game_build) !== String(build) ||
    Number(diagnostic?.processing?.sample_count) !== 735016 ||
    diagnostic?.processing?.measured_peak_within_configured_limit !== true ||
    String(enrichment?.sha256).toLowerCase() !== `sha256:${identityDescriptor.sha256}` ||
    Number(enrichment?.bytes) !== identityDescriptor.bytes ||
    Number(enrichment?.exact_formula_cohort_sample_joins) !== 774 ||
    Number(enrichment?.exact_formula_cohort_source_actor_joins) !== 773 ||
    Number(enrichment?.exact_formula_cohort_scene_joins) !== 764 ||
    Number(enrichment?.formula_cohort_identity_conflicts) !== 0 ||
    diagnostic?.policy?.remote_player_only_packets_are_required !== false ||
    diagnostic?.policy
      ?.actor_identity_is_the_most_recent_packet_observed_actor_event_not_a_current_character_snapshot !== true ||
    diagnostic?.authority?.exact_target_mitigation_formula_proven !== false ||
    diagnostic?.authority?.exact_operation_order_and_integer_rounding_proven !== false ||
    diagnostic?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("neutral mitigation actor-scene diagnostic is unsafe or incomplete");
  }
  return {
    actions: 774,
    player_targets: 774,
    source_actors: 773,
    scenes: 764,
    physical_defense_actor_scene_contexts: 747,
    magic_defense_actor_scene_contexts: 747,
    refined_defense_actor_scene_contexts: 756,
    controlled_axis_pairs: 0,
    target_allegiance_assumed: false,
    current_actor_snapshots_substituted: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validatePacketEvidence(proof, build) {
  if (proof?.content_sha256 !== contentHash(proof) || String(proof?.game_build) !== build) {
    throw new Error("packet target-mitigation proof has invalid build identity or content hash");
  }
  if (proof?.schema_version === 2 && proof?.generated_by === SINGLE_PACKET_PROOF_GENERATOR) {
    if (proof?.policy?.runtime_authority !== false || proof?.policy?.formula_authority !== false ||
      proof?.policy?.unresolved_evidence_is_hidden !== false ||
      proof?.policy?.disk_partitions_preserve_exact_group_semantics !== true ||
      proof?.policy?.cross_capture_pairing_allowed !== false ||
      !Array.isArray(proof?.input?.source_inputs) || proof.input.source_inputs.length === 0 ||
      Number(proof?.processing?.sample_count) <= 0 ||
      Number(proof?.processing?.measured_peak_working_set_bytes) <= 0) {
      throw new Error("single-capture packet target-mitigation proof violates the fail-closed policy");
    }
    return {
      artifact_kind: "single-capture-proof",
      matching_build_capture_proofs: 1,
      matching_build_source_rlogs: proof.input.source_inputs.length,
      cohort_input_bytes: Number(proof.input.bytes),
      damage_samples: Number(proof.processing.sample_count),
      audited_axis_samples: sumAxisCounter(proof, "samples_with_axis"),
      controlled_groups: sumAxisCounter(proof, "controlled_groups"),
      deterministic_pairs: sumAxisCounter(proof, "deterministic_pairs"),
      divergent_output_pairs: sumAxisCounter(proof, "divergent_output_pairs"),
      maximum_measured_peak_working_set_bytes:
        Number(proof.processing.measured_peak_working_set_bytes),
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    };
  }
  if (proof?.schema_version === 1 && proof?.generated_by === PACKET_ROLLUP_GENERATOR) {
    if (proof?.policy?.exact_input_hashes_are_embedded_and_verified !== true ||
      proof?.policy?.every_capture_is_analyzed_independently !== true ||
      proof?.policy?.cross_capture_pairing_allowed !== false ||
      proof?.policy?.bounded_memory_measurement_required_for_every_input !== true ||
      proof?.policy?.absence_of_controlled_pairs_is_not_formula_proof !== true ||
      proof?.policy?.unresolved_evidence_is_preserved !== true ||
      proof?.policy?.formula_authority !== false || proof?.policy?.runtime_authority !== false ||
      proof?.policy?.provider_rdps_credit_allowed !== false ||
      !Array.isArray(proof?.runs) || proof.runs.length === 0 ||
      Number(proof?.summary?.matching_build_capture_proofs) !== proof.runs.length ||
      Number(proof?.summary?.matching_build_source_rlogs) < proof.runs.length ||
      Number(proof?.summary?.maximum_measured_peak_working_set_bytes) <= 0 ||
      Number(proof?.summary?.controlled_groups) !== 0 ||
      Number(proof?.summary?.deterministic_pairs) !== 0 ||
      Number(proof?.summary?.divergent_output_pairs) !== 0 ||
      proof?.summary?.exact_target_mitigation_formula_proven !== false ||
      proof?.summary?.operation_order_and_integer_rounding_proven !== false ||
      proof?.summary?.packet_conservation_proven !== false ||
      proof?.summary?.formula_authority !== false || proof?.summary?.runtime_authority !== false ||
      proof?.summary?.provider_rdps_credit_allowed !== false ||
      proof?.status !== "no-controlled-target-mitigation-pairs") {
      throw new Error("packet target-mitigation rollup violates the fail-closed policy");
    }
    return {
      artifact_kind: "matching-build-formula-cohort-rollup",
      matching_build_capture_proofs: Number(proof.summary.matching_build_capture_proofs),
      matching_build_source_rlogs: Number(proof.summary.matching_build_source_rlogs),
      cohort_input_bytes: Number(proof.summary.cohort_input_bytes),
      damage_samples: Number(proof.summary.damage_samples),
      audited_axis_samples: Number(proof.summary.audited_axis_samples),
      controlled_groups: Number(proof.summary.controlled_groups),
      deterministic_pairs: Number(proof.summary.deterministic_pairs),
      divergent_output_pairs: Number(proof.summary.divergent_output_pairs),
      maximum_measured_peak_working_set_bytes:
        Number(proof.summary.maximum_measured_peak_working_set_bytes),
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    };
  }
  throw new Error("unsupported packet target-mitigation proof schema or generator");
}

function sumAxisCounter(proof, counter) {
  return Object.values(proof?.axes || {}).reduce((sum, axis) => {
    const value = Number(axis?.counters?.[counter]);
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new Error(`packet target-mitigation axis has invalid ${counter}`);
    }
    return sum + value;
  }, 0);
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    parsed[key] = value;
  }
  for (const required of [
    "gameBuild", "packetBuild", "luaDefinitionAudit", "luaConsumerAudit", "directCallsiteAudit",
    "fightAttributeProof", "packetPairProof", "neutralActionIdentity",
    "neutralMitigationDiagnostic", "output",
  ]) {
    if (!parsed[required]) throw new Error(`missing --${required}`);
  }
  return parsed;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`failed to read ${label} at ${filePath}: ${error.message}`);
  }
}

function relative(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}

function fileDescriptor(filePath) {
  const bytes = readFileSync(filePath);
  return {
    path: relative(filePath),
    bytes: statSync(filePath).size,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function verifyResult(report) {
  if (report?.schema_version !== SCHEMA_VERSION ||
    report?.generated_by !== GENERATOR ||
    report?.content_sha256 !== contentHash(report) ||
    report?.policy?.exact_build_required !== true ||
    report?.policy?.exact_input_hashes_are_embedded !== true ||
    report?.policy?.unresolved_evidence_is_hidden !== false ||
    report?.policy?.candidate_constants_are_combat_formula_authority !== false ||
    report?.policy?.character_sheet_transform_is_combat_formula_authority !== false ||
    report?.policy?.absence_of_direct_calls_proves_absence_of_indirect_consumers !== false ||
    report?.policy?.no_formula_is_promoted_without_controlled_packet_counterfactuals !== true ||
    report?.policy?.matching_build_packet_validation_is_required !== true ||
    typeof report?.policy?.matching_build_formula_cohort_rollup_is_bound !== "boolean" ||
    report?.policy?.rollup_proves_repository_wide_capture_completeness !== false ||
    report?.policy?.mitigation_action_targets_are_allegiance_neutral !== true ||
    report?.policy?.current_actor_snapshots_are_never_substituted !== true ||
    report?.policy?.exact_client_combat_damage_consumer_proven !== false ||
    report?.policy?.server_combat_implementation_is_available_in_client_files !== false ||
    Number(report?.summary?.packet_axes_audited) !== 12 ||
    Number(report?.summary?.controlled_counterfactual_pairs) !== 0 ||
    Number(report?.summary?.packet_samples_with_physical_or_refined_defense) <= 0 ||
    Number(report?.summary?.packet_samples_with_magic_defense) < 0 ||
    Number(report?.summary?.neutral_mitigation_actions) !== 774 ||
    Number(report?.summary?.neutral_player_targets) !== 774 ||
    Number(report?.summary?.event_time_damage_actors) !== 773 ||
    Number(report?.summary?.event_time_run_scenes) !== 764 ||
    Number(report?.summary?.actor_scene_controlled_axis_pairs) !== 0 ||
    Number(report?.summary?.promoted_combat_formulas) !== 0 ||
    !Array.isArray(report?.final_validation) || report.final_validation.length !== 2) {
    throw new Error("target mitigation offline exhaustion proof failed its fail-closed verifier");
  }
  const scope = report.packet_evidence_scope;
  if (!scope || !["single-capture-proof", "matching-build-formula-cohort-rollup"].includes(scope.artifact_kind) ||
    (scope.artifact_kind === "matching-build-formula-cohort-rollup") !==
      report.policy.matching_build_formula_cohort_rollup_is_bound ||
    Number(scope.matching_build_capture_proofs) <= 0 ||
    Number(scope.matching_build_source_rlogs) < Number(scope.matching_build_capture_proofs) ||
    Number(scope.damage_samples) <= 0 || Number(scope.audited_axis_samples) <= 0 ||
    Number(scope.maximum_measured_peak_working_set_bytes) <= 0 ||
    Number(scope.controlled_groups) !== 0 || Number(scope.deterministic_pairs) !== 0 ||
    Number(scope.divergent_output_pairs) !== 0 || scope.formula_authority !== false ||
    scope.runtime_authority !== false || scope.provider_rdps_credit_allowed !== false) {
    throw new Error("target mitigation offline exhaustion packet evidence scope is invalid");
  }
  for (const descriptor of Object.values(report.inputs ?? {})) {
    const filePath = resolvePath(descriptor?.path);
    if (!descriptor || statSync(filePath).size !== Number(descriptor.bytes) ||
      createHash("sha256").update(readFileSync(filePath)).digest("hex") !== descriptor.sha256) {
      throw new Error(`target mitigation offline exhaustion input changed: ${descriptor?.path}`);
    }
  }
}
