#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 2;
const EXPECTED_BUILD = "24687926";
const EXPECTED_EFFECT_ID = 31_602;
const EXPECTED_SKILL_TABLE_BYTES = 11_942_173;
const EXPECTED_SKILL_TABLE_SHA256 =
  "2cb172bc819491b45e9bb160be7387dbb0f5107e04663cf67f19013da0403f75";
const EXPECTED_SOURCE_MANIFEST_AGGREGATE_SHA256 =
  "b51f6b0db367b0aad2883e2626ddaf0b5b663b5cc9e7b17d981e764e4c6f67d4";
const EXPECTED_STAGE_CATALOG_BYTES = 23_166_253;
const EXPECTED_STAGE_CATALOG_SHA256 =
  "807f92cf3fd3e53bbdc6fae75e18e01f623aad8bb34b363da1bb493549194376";
const NORMAL_LANE = "normal_attack_speed_attr_11720_plus_temporary_700";
const GUIDE_LANE = "guide_speed_attr_11730_plus_temporary_710";

function fail(message) {
  throw new Error(message);
}

function take(values, flag) {
  const index = values.indexOf(flag);
  if (index < 0 || index + 1 >= values.length) fail(`${flag} requires a value`);
  const value = values[index + 1];
  values.splice(index, 2);
  return value;
}

function parseArguments(argv) {
  const values = [...argv];
  const command = values.shift();
  if (command === "verify") {
    const input = path.resolve(take(values, "--input"));
    if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
    return { command, input };
  }
  if (command === "self-test") {
    if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
    return { command };
  }
  if (command !== "generate") fail("expected generate, verify, or self-test");
  const result = {
    command,
    build: take(values, "--build"),
    timingProof: path.resolve(take(values, "--timing-proof")),
    stageCatalog: path.resolve(take(values, "--stage-catalog")),
    skillTable: path.resolve(take(values, "--skill-table")),
    sourceManifest: path.resolve(take(values, "--source-manifest")),
    actionSpeedProof: path.resolve(take(values, "--action-speed-proof")),
    output: path.resolve(take(values, "--output")),
  };
  if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
  return result;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function receipt(file, bytes) {
  return { path: file, bytes: statSync(file).size, sha256: sha256(bytes) };
}

function parseJson(bytes) {
  return JSON.parse(bytes.toString("utf8").replace(/^\uFEFF/, ""));
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return sha256(Buffer.from(JSON.stringify(copy)));
}

function integer(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number)) fail(`${label} must be a safe integer`);
  return number;
}

function validateInputs({ build, timing, stageCatalog, skillTable, sourceManifest, actionSpeed }) {
  if (
    build !== EXPECTED_BUILD ||
    Number(timing?.schema_version) !== 13 ||
    timing?.generated_by !== "tools/bpsr-action-hit-timing-ancestry-proof.mjs" ||
    timing?.game_build !== build ||
    Number(timing?.effect_id) !== EXPECTED_EFFECT_ID ||
    Number(timing?.summary?.responsive_damage_event_join_memberships) !== 3_713 ||
    BigInt(timing?.summary?.responsive_damage_event_join_reported_damage_units ?? "-1") !==
      556_129_992n ||
    timing?.summary?.provider_rdps_credit_allowed !== false ||
    timing?.summary?.observed_damage_reassigned_to_provider !== 0 ||
    Number(actionSpeed?.schema_version) !== 5 ||
    actionSpeed?.game_build !== build ||
    actionSpeed?.summary?.exact_native_float32_operation_order_proven !== true ||
    actionSpeed?.summary?.singing_native_float32_operation_order_proven !== true ||
    actionSpeed?.summary?.singing_offline_numeric_equivalence_proven !== false ||
    actionSpeed?.policy?.provider_rdps_credit_allowed !== false ||
    Number(actionSpeed?.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    Number(sourceManifest?.schemaVersion) !== 1 ||
    sourceManifest?.generatedBy !== "tools/bpsr-build-source-manifest.mjs" ||
    sourceManifest?.gameBuild !== build ||
    sourceManifest?.authority?.decodedGameTables !== "exact-current-build-static-data" ||
    sourceManifest?.coverage?.complete !== true ||
    Number(sourceManifest?.coverage?.silentOmissions) !== 0 ||
    sourceManifest?.aggregateSha256 !== EXPECTED_SOURCE_MANIFEST_AGGREGATE_SHA256 ||
    Number(stageCatalog?.schema_version) !== 3 ||
    stageCatalog?.generated_by !== "tools/bpsr-skill-logic-decoder" ||
    stageCatalog?.build !== build ||
    stageCatalog?.authority?.exact_build_skill_logic_payload_decoded !== true ||
    stageCatalog?.authority?.stage_logic_member_order_proven !== true ||
    stageCatalog?.authority?.runtime_promotion_allowed !== false ||
    Number(stageCatalog?.summary?.skill_stage_rows) !== 9_142 ||
    Number(stageCatalog?.summary?.unresolved_stage_event_parameter_references) !== 0
  ) {
    fail("inputs are not the exact fail-closed current-build speed join frontier");
  }
  const manifestRow = (sourceManifest.files ?? []).find(
    (row) => row.id === "decoded-game-tables:SkillTable.json",
  );
  if (
    manifestRow?.authority !== "exact-current-build-static-data" ||
    manifestRow?.relativePath !== "SkillTable.json" ||
    Number(manifestRow?.bytes) !== EXPECTED_SKILL_TABLE_BYTES ||
    manifestRow?.sha256 !== EXPECTED_SKILL_TABLE_SHA256 ||
    !skillTable ||
    Array.isArray(skillTable) ||
    Object.keys(skillTable).length !== 4_856
  ) {
    fail("SkillTable is not the exact manifest-authorized current-build table");
  }
}

function classifyGroup(group, skillTable, stageIndex) {
  const actionId = integer(group.action_id, "action id");
  const dictionaryKey = integer(group.dictionary_key, "dictionary key");
  const memberships = integer(group.damage_action_memberships, "group memberships");
  const damage = BigInt(group.reported_damage_units);
  if (memberships <= 0 || damage < 0n || group.provider_rdps_credit_allowed !== false) {
    fail("timing group is inconsistent or unsafe");
  }
  const skill = skillTable[String(actionId)];
  const stage = stageIndex.get(`${group.dictionary_kind}:${dictionaryKey}:${group.packet_owner_stage}`);
  let resolution;
  if (!skill) {
    resolution = "unresolved_no_exact_skill_table_row";
  } else if (!(skill.EffectIDs ?? []).map(String).includes(String(dictionaryKey))) {
    resolution = "unresolved_dictionary_key_not_in_skill_effect_ids";
  } else if (!stage) {
    resolution = "unresolved_no_exact_stage_index_row";
  } else if (
    group.speed_lane === NORMAL_LANE &&
    skill.AtkSpeedSwitch === true &&
    Number(stage.stage_type) === 0
  ) {
    resolution = "exact_normal_lane_skill_effect_and_atk_speed_switch_enabled";
  } else if (group.speed_lane === NORMAL_LANE) {
    resolution = "unresolved_normal_lane_skill_switch_or_stage_type_mismatch";
  } else if (
    group.speed_lane === GUIDE_LANE &&
    (Number(stage.stage_type) === 8 || Number(stage.stage_type) === 9)
  ) {
    resolution = "exact_guide_lane_skill_effect_and_native_stage_type";
  } else {
    resolution = "unresolved_skill_effect_speed_lane_or_stage_type_mismatch";
  }
  return {
    action_id: actionId,
    dictionary_kind: group.dictionary_kind,
    dictionary_key: dictionaryKey,
    damage_attr_id: String(group.damage_attr_id),
    packet_owner_stage: integer(group.packet_owner_stage, "packet owner stage"),
    speed_lane: group.speed_lane,
    skill_table_resolution: resolution,
    skill_table_row_present: Boolean(skill),
    dictionary_key_in_skill_effect_ids: Boolean(
      skill && (skill.EffectIDs ?? []).map(String).includes(String(dictionaryKey)),
    ),
    skill_table_atk_speed_switch: skill ? skill.AtkSpeedSwitch === true : null,
    exact_stage_index_row_present: Boolean(stage),
    native_stage_type: stage ? integer(stage.stage_type, "native stage type") : null,
    exact_packet_owner_stage_to_native_stage_type_proven: Boolean(
      skill &&
        (skill.EffectIDs ?? []).map(String).includes(String(dictionaryKey)) &&
        stage &&
        (resolution === "exact_normal_lane_skill_effect_and_atk_speed_switch_enabled" ||
          resolution === "exact_guide_lane_skill_effect_and_native_stage_type"),
    ),
    formula_authority: false,
    provider_rdps_credit_allowed: false,
    damage_action_memberships: memberships,
    reported_damage_units: damage.toString(),
  };
}

function summarize(rows) {
  const buckets = new Map();
  let totalMemberships = 0;
  let totalDamage = 0n;
  for (const row of rows) {
    totalMemberships += row.damage_action_memberships;
    totalDamage += BigInt(row.reported_damage_units);
    const bucket = buckets.get(row.skill_table_resolution) ?? {
      groups: 0,
      memberships: 0,
      damage: 0n,
    };
    bucket.groups += 1;
    bucket.memberships += row.damage_action_memberships;
    bucket.damage += BigInt(row.reported_damage_units);
    buckets.set(row.skill_table_resolution, bucket);
  }
  const count = (resolution) => {
    const value = buckets.get(resolution) ?? { groups: 0, memberships: 0, damage: 0n };
    return {
      groups: value.groups,
      memberships: value.memberships,
      reported_damage_units: value.damage.toString(),
    };
  };
  const exactResolutions = [
    "exact_normal_lane_skill_effect_and_atk_speed_switch_enabled",
    "exact_guide_lane_skill_effect_and_native_stage_type",
  ];
  const exact = exactResolutions.reduce(
    (sum, resolution) => {
      const value = buckets.get(resolution);
      if (value) {
        sum.groups += value.groups;
        sum.memberships += value.memberships;
        sum.damage += value.damage;
      }
      return sum;
    },
    { groups: 0, memberships: 0, damage: 0n },
  );
  return {
    responsive_groups: rows.length,
    responsive_memberships: totalMemberships,
    responsive_reported_damage_units: totalDamage.toString(),
    exact_skill_table_effect_join_groups: exact.groups,
    exact_skill_table_effect_join_memberships: exact.memberships,
    exact_skill_table_effect_join_reported_damage_units: exact.damage.toString(),
    exact_normal_lane_atk_speed_switch_enabled: count(
      "exact_normal_lane_skill_effect_and_atk_speed_switch_enabled",
    ),
    unresolved_normal_lane_skill_switch_or_stage_type_mismatch: count(
      "unresolved_normal_lane_skill_switch_or_stage_type_mismatch",
    ),
    exact_guide_lane_native_stage_type: count(
      "exact_guide_lane_skill_effect_and_native_stage_type",
    ),
    unresolved_dictionary_key_not_in_skill_effect_ids: count(
      "unresolved_dictionary_key_not_in_skill_effect_ids",
    ),
    unresolved_no_exact_skill_table_row: count("unresolved_no_exact_skill_table_row"),
    unresolved_no_exact_stage_index_row: count("unresolved_no_exact_stage_index_row"),
    current_build_skill_table_identity_proven: true,
    exact_native_speed_float32_operation_order_proven: true,
    exact_packet_owner_stage_to_native_stage_type_proven_for_exact_skill_rows:
      exact.memberships > 0,
    exact_action_opportunity_proven: false,
    packet_conservation_proven: false,
    classification_conservation_proven: true,
    provider_rdps_credit_allowed: false,
    ui_rdps_display_allowed: false,
    runtime_promotion_allowed: false,
    observed_damage_reassigned_to_provider: 0,
  };
}

function buildReport({ build, timing, stageCatalog, skillTable, sourceManifest, actionSpeed, inputs }) {
  validateInputs({ build, timing, stageCatalog, skillTable, sourceManifest, actionSpeed });
  const stageIndex = new Map();
  for (const row of [
    ...(stageCatalog.skill_stages ?? []),
    ...(stageCatalog.bullet_stages ?? []),
    ...(stageCatalog.buff_stages ?? []),
  ]) {
    const key = `${row.dictionary_kind}:${row.dictionary_key}:${row.stage_index}`;
    if (stageIndex.has(key)) fail(`duplicate exact stage-index key ${key}`);
    stageIndex.set(key, row);
  }
  const rows = (timing.action_damage_event_timing_groups ?? []).map((group) =>
    classifyGroup(group, skillTable, stageIndex),
  );
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-action-speed-skill-table-join-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    game_build: build,
    effect_id: EXPECTED_EFFECT_ID,
    proof_state:
      "exact-current-build-skill-effect-and-atk-speed-switch-join-proven-owner-stage-runtime-join-open",
    inputs,
    relationship_model: {
      provider_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      selected_effect_endpoint_damage_role: "damage_actor",
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      remote_player_cast_packets_required: false,
      missing_skill_table_rows_are_synthesized: false,
      packet_owner_stage_is_native_stage_type_without_proof: false,
      ordinary_damage_totals_unchanged: true,
      provider_rdps_credit_allowed: false,
      ui_rdps_display_allowed: false,
    },
    rows,
    summary: summarize(rows),
    blockers: [
      "five bullet-root groups do not have an exact SkillTable effect join and remain unresolved",
      "remote action-start packets are unavailable by transport design; action opportunity must be reconstructed from native scheduling, damage ancestry, and clock correspondence",
      "exact offline float32 replay, integer damage rounding, protocol-pack identity, and required replay gates remain open",
    ],
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  return report;
}

function validateReport(report) {
  const rows = report?.rows ?? [];
  const expected = summarize(rows);
  if (
    Number(report?.schema_version) !== SCHEMA_VERSION ||
    report?.generated_by !== "tools/bpsr-action-speed-skill-table-join-proof.mjs" ||
    report?.game_build !== EXPECTED_BUILD ||
    Number(report?.effect_id) !== EXPECTED_EFFECT_ID ||
    JSON.stringify(report?.summary) !== JSON.stringify(expected) ||
    report?.policy?.missing_skill_table_rows_are_synthesized !== false ||
    report?.policy?.packet_owner_stage_is_native_stage_type_without_proof !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    report?.summary?.provider_rdps_credit_allowed !== false ||
    report?.summary?.ui_rdps_display_allowed !== false ||
    report?.summary?.runtime_promotion_allowed !== false ||
    Number(report?.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    report?.content_sha256 !== contentHash(report)
  ) {
    fail("SkillTable speed join proof is inconsistent or unsafe");
  }
}

function generate(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports build ${EXPECTED_BUILD}`);
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const timingBytes = readFileSync(options.timingProof);
  const stageBytes = readFileSync(options.stageCatalog);
  const tableBytes = readFileSync(options.skillTable);
  const manifestBytes = readFileSync(options.sourceManifest);
  const speedBytes = readFileSync(options.actionSpeedProof);
  if (
    tableBytes.length !== EXPECTED_SKILL_TABLE_BYTES ||
    sha256(tableBytes) !== EXPECTED_SKILL_TABLE_SHA256
  ) {
    fail("SkillTable bytes do not match the reviewed exact current build");
  }
  if (
    stageBytes.length !== EXPECTED_STAGE_CATALOG_BYTES ||
    sha256(stageBytes) !== EXPECTED_STAGE_CATALOG_SHA256
  ) {
    fail("stage catalog bytes do not match the reviewed exact current build");
  }
  const report = buildReport({
    build: options.build,
    timing: parseJson(timingBytes),
    stageCatalog: parseJson(stageBytes),
    skillTable: parseJson(tableBytes),
    sourceManifest: parseJson(manifestBytes),
    actionSpeed: parseJson(speedBytes),
    inputs: {
      action_damage_event_timing_ancestry: receipt(options.timingProof, timingBytes),
      exact_current_build_stage_logic_catalog: receipt(options.stageCatalog, stageBytes),
      exact_current_build_skill_table: receipt(options.skillTable, tableBytes),
      complete_build_source_manifest: receipt(options.sourceManifest, manifestBytes),
      native_action_speed_formula: receipt(options.actionSpeedProof, speedBytes),
    },
  });
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    `proved ${report.summary.exact_skill_table_effect_join_groups} exact SkillTable effect joins covering ${report.summary.exact_skill_table_effect_join_memberships} memberships; provider credit=false\nwrote ${options.output}\n`,
  );
}

function selfTest() {
  const rows = [
    {
      action_id: 1,
      dictionary_kind: "skill",
      dictionary_key: 101,
      damage_attr_id: "1001",
      packet_owner_stage: 0,
      speed_lane: NORMAL_LANE,
      provider_rdps_credit_allowed: false,
      damage_action_memberships: 2,
      reported_damage_units: "30",
    },
    {
      action_id: 2,
      dictionary_kind: "skill",
      dictionary_key: 201,
      damage_attr_id: "2001",
      packet_owner_stage: 1,
      speed_lane: GUIDE_LANE,
      provider_rdps_credit_allowed: false,
      damage_action_memberships: 3,
      reported_damage_units: "40",
    },
  ].map((row) => classifyGroup(
    row,
    {
      1: { EffectIDs: [101], AtkSpeedSwitch: true },
      2: { EffectIDs: [201], AtkSpeedSwitch: false },
    },
    new Map([
      ["skill:101:0", { stage_type: 0 }],
      ["skill:201:1", { stage_type: 8 }],
    ]),
  ));
  const summary = summarize(rows);
  if (
    summary.responsive_memberships !== 5 ||
    summary.responsive_reported_damage_units !== "70" ||
    summary.exact_skill_table_effect_join_groups !== 2 ||
    summary.provider_rdps_credit_allowed !== false
  ) {
    fail("self-test classification mismatch");
  }
  const unresolved = classifyGroup(
    {
      action_id: 3,
      dictionary_kind: "bullet",
      dictionary_key: 301,
      damage_attr_id: "3001",
      packet_owner_stage: 0,
      speed_lane: NORMAL_LANE,
      provider_rdps_credit_allowed: false,
      damage_action_memberships: 1,
      reported_damage_units: "1",
    },
    {},
    new Map(),
  );
  if (
    unresolved.skill_table_resolution !== "unresolved_no_exact_skill_table_row" ||
    unresolved.provider_rdps_credit_allowed !== false
  ) {
    fail("self-test synthesized a missing SkillTable row");
  }
  process.stdout.write("SkillTable speed join self-test passed; provider credit=false\n");
}

const options = parseArguments(process.argv.slice(2));
if (options.command === "generate") generate(options);
else if (options.command === "self-test") selfTest();
else {
  const report = JSON.parse(readFileSync(options.input));
  validateReport(report);
  process.stdout.write(
    `verified ${report.summary.exact_skill_table_effect_join_groups} exact SkillTable effect joins; provider credit=false\n`,
  );
}
