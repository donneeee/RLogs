#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATOR = "tools/bpsr-target-mitigation-actor-scene-exhaustion.mjs";
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "analyze") analyze(options);
else if (command === "verify") verify(options);
else usage(command === "help" ? 0 : 1);

function analyze(parsed) {
  const build = required(parsed, "build");
  const cohortPath = path.resolve(required(parsed, "cohort"));
  const diagnosticPath = path.resolve(required(parsed, "diagnostic"));
  const nearPairPath = path.resolve(required(parsed, "near-pair-evidence"));
  const output = path.resolve(required(parsed, "output"));
  const cohort = readJson(cohortPath);
  const diagnostic = readJson(diagnosticPath);
  const nearPair = readJson(nearPairPath);
  const report = buildReport(build, cohort, diagnostic, nearPair, {
    cohort: fileDescriptor(cohortPath),
    cross_capture_actor_shape_diagnostic: fileDescriptor(diagnosticPath),
    same_capture_near_pair_evidence: fileDescriptor(nearPairPath),
  });
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(`wrote ${output}`);
}

function buildReport(build, cohort, diagnostic, nearPair, inputs) {
  if (Number(cohort?.schema_version) !== 41 ||
    cohort?.generated_by !== "rlogs-bpsr-state-scaling-damage-proof" ||
    String(cohort?.game_build) !== build ||
    cohort?.policy?.formula_authority !== false) {
    throw new Error("Unsupported or authoritative actor-scene cohort");
  }
  const samples = cohort.samples ?? [];
  if (samples.length !== 185 || (cohort.inputs ?? []).length !== 26 ||
    samples.some((sample) => Number(sample.ability_id) !== 823225)) {
    throw new Error("Actor-scene cohort scope changed");
  }
  const attributeStates = cohort.attribute_states ?? [];
  const physicalDefenseSamples = samples.filter((sample) =>
    (attributeStates[Number(sample.target_attribute_state_id)] ?? [])
      .some((row) => Number(row.attribute_id) === 11350));
  const withScene = samples.filter((sample) => Number.isSafeInteger(Number(sample.scene_id))).length;
  const withStableSource = samples.filter((sample) => {
    const actor = sample.source_actor_identity;
    return actor && (actor.character_id != null || actor.monster_id != null);
  }).length;
  const withTargetActor = samples.filter((sample) => sample.target_actor_identity != null).length;
  const withStableTarget = samples.filter((sample) => {
    const actor = sample.target_actor_identity;
    return actor && (actor.character_id != null || actor.monster_id != null);
  }).length;
  if (withScene !== 185 || withStableSource !== 185 || withTargetActor !== 185 ||
    withStableTarget !== 0 || physicalDefenseSamples.length !== 23) {
    throw new Error("Actor-scene coverage changed");
  }

  const physical = diagnostic?.axes?.physical_defense;
  const counters = physical?.counters;
  if (Number(diagnostic?.schema_version) !== 3 ||
    diagnostic?.generated_by !==
      "rlogs-bpsr-target-mitigation-transform-proof:cross-capture-target-config-diagnostic" ||
    String(diagnostic?.game_build) !== build ||
    diagnostic?.authority?.formula_authority !== false ||
    diagnostic?.authority?.runtime_authority !== false ||
    diagnostic?.authority?.provider_rdps_credit_allowed !== false ||
    diagnostic?.input?.sha256 !== `sha256:${inputs.cohort.sha256}` ||
    Number(counters?.samples_with_axis) !== 23 ||
    Number(counters?.samples_with_cross_capture_actor_shape_context) !== 23 ||
    Number(counters?.samples_with_stable_target_actor_id) !== 0 ||
    Number(counters?.pairs_with_cross_capture_witness) !== 0 ||
    Number(counters?.deterministic_cross_capture_pairs) !== 0 ||
    Number(physical?.models?.transformed_curve?.counters?.exact_pairs) !== 0 ||
    Number(physical?.models?.runtime_simple_curve?.counters?.exact_pairs) !== 0) {
    throw new Error("Cross-capture actor-shape diagnostic changed");
  }

  if (Number(nearPair?.schema_version) !== 3 ||
    nearPair?.generated_by !== "tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs" ||
    String(nearPair?.game_build) !== build ||
    nearPair?.status !== "exact-integer-candidate-compatible-status-confounded" ||
    Number(nearPair?.exact_candidate_evaluation?.packet_near_pair_rows) !== 3 ||
    Number(nearPair?.exact_candidate_evaluation?.transformed_curve_compatible_rows) !== 3 ||
    Number(nearPair?.exact_candidate_evaluation?.runtime_simple_curve_compatible_rows) !== 0 ||
    nearPair?.confounders?.same_axis_status_invariance
      ?.candidate_near_pair_remains_confounded !== true ||
    nearPair?.authority?.formula_authority !== false ||
    nearPair?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("Same-capture near-pair evidence changed");
  }

  const defenseSessions = [...new Set(physicalDefenseSamples.map((sample) => sample.session_id))];
  const defenseScenes = [...new Set(physicalDefenseSamples.map((sample) => Number(sample.scene_id)))];
  const sourceMonsterIds = [...new Set(physicalDefenseSamples.map(
    (sample) => Number(sample.source_actor_identity?.monster_id),
  ))];
  const targetActorShapes = [...new Set(physicalDefenseSamples.map((sample) => JSON.stringify({
    entity_type_id: sample.target_actor_identity?.entity_type_id ?? null,
    class_id: sample.target_actor_identity?.class_id ?? null,
    specialization_id: sample.target_actor_identity?.specialization_id ?? null,
    level: sample.target_actor_identity?.level ?? null,
  })))].map(JSON.parse);

  return {
    schema_version: 1,
    generated_by: GENERATOR,
    game_build: build,
    model_id: "target-physical-armor-counterfactual",
    status: "exact-local-actor-scene-exhausted-no-cross-capture-control",
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      only_local_capture_and_exact_build_offline_evidence_are_required: true,
      structurally_unavailable_remote_player_packets_are_not_required: true,
      missing_stable_remote_player_identity_is_preserved_not_synthesized: true,
      actor_shape_grouping_is_diagnostic_only: true,
      same_capture_status_confounders_are_not_ignored: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs,
    summary: {
      exact_build_source_rlogs: 26,
      selected_ability_id: 823225,
      selected_ability_samples: 185,
      physical_defense_samples: 23,
      physical_defense_capture_sessions: defenseSessions.length,
      physical_defense_scene_ids: defenseScenes,
      exact_source_monster_ids: sourceMonsterIds,
      target_actor_shapes: targetActorShapes,
      physical_defense_samples_with_stable_target_actor_id: 0,
      cross_capture_actor_shape_pairs: 0,
      same_capture_status_confounded_near_pair_rows: 3,
      transformed_curve_22000_compatible_status_confounded_rows: 3,
      runtime_simple_curve_6500_compatible_rows: 0,
      exact_target_mitigation_formula_proven: false,
      packet_conservation_proven: false,
      provider_rdps_credit_allowed: false,
    },
    blockers: [
      "all packet-observed physical-defense rows for ability 823225 occur in one capture session, so no cross-capture armor variation exists",
      "the same-capture 5907 versus 5370 defense rows remain target-status confounded",
      "the player target has packet-observed entity type, class, and specialization but no stable character ID or level; this absence is not repaired with unavailable remote-player packets",
      "exact armor-to-damage operation order, integer rounding, and canonical damage conservation remain unproven for build 24687926",
    ],
    authority: {
      exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
}

function verify(parsed) {
  const input = path.resolve(required(parsed, "input"));
  const report = readJson(input);
  verifyReport(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  const expected = contentHash(report);
  if (report?.schema_version !== 1 || report?.generated_by !== GENERATOR ||
    report?.status !== "exact-local-actor-scene-exhausted-no-cross-capture-control" ||
    report?.policy?.structurally_unavailable_remote_player_packets_are_not_required !== true ||
    report?.policy?.missing_stable_remote_player_identity_is_preserved_not_synthesized !== true ||
    report?.summary?.physical_defense_samples !== 23 ||
    report?.summary?.physical_defense_capture_sessions !== 1 ||
    report?.summary?.cross_capture_actor_shape_pairs !== 0 ||
    report?.summary?.same_capture_status_confounded_near_pair_rows !== 3 ||
    report?.authority?.formula_authority !== false ||
    report?.authority?.runtime_authority !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false ||
    report?.content_sha256 !== expected) {
    throw new Error("Unsafe or inconsistent actor-scene exhaustion report");
  }
  for (const descriptor of Object.values(report.inputs ?? {})) {
    if (!Number.isSafeInteger(descriptor?.bytes) || descriptor.bytes <= 0 ||
      !/^[0-9a-f]{64}$/.test(String(descriptor?.sha256 ?? ""))) {
      throw new Error("Invalid input descriptor");
    }
  }
}

function fileDescriptor(file) {
  const bytes = statSync(file).size;
  const sha256 = createHash("sha256").update(readFileSync(file)).digest("hex");
  return { path: file.replaceAll("\\", "/"), bytes, sha256 };
}

function contentHash(report) {
  const clone = structuredClone(report);
  delete clone.content_sha256;
  return createHash("sha256").update(JSON.stringify(clone)).digest("hex");
}

function readJson(file) {
  return JSON.parse(readFileSync(file, "utf8"));
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value == null) usage(1);
    parsed[key.slice(2)] = value;
  }
  return parsed;
}

function required(parsed, key) {
  if (!parsed[key]) throw new Error(`Missing --${key}`);
  return parsed[key];
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-target-mitigation-actor-scene-exhaustion.mjs analyze --build <id> --cohort <json> --diagnostic <json> --near-pair-evidence <json> --output <json>\n  node tools/bpsr-target-mitigation-actor-scene-exhaustion.mjs verify --input <json>");
  process.exit(exitCode);
}
