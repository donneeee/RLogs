#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-training-scene-access-frontier.mjs";
const GAME_BUILD = "24687926";
const TRAINING_SCENE_IDS = [10001, 10002];

function fail(message) {
  throw new Error(message);
}

function readJson(file, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    fail(`Cannot read ${label} ${file}: ${error.message}`);
  }
}

async function descriptor(file) {
  const hash = crypto.createHash("sha256");
  let bytes = 0;
  for await (const chunk of fs.createReadStream(file)) {
    bytes += chunk.length;
    hash.update(chunk);
  }
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes,
    sha256: hash.digest("hex"),
  };
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(canonical(copy)).digest("hex").toUpperCase();
}

function sortedUnique(values) {
  return [...new Set(values)].sort((left, right) => left.localeCompare(right));
}

function luaFilesMatching(audit, predicate) {
  const files = [];
  for (const file of audit.files ?? []) {
    if ((file.matches ?? []).some(predicate)) files.push(path.basename(file.file));
  }
  return sortedUnique(files);
}

function validateLuaAudit(audit) {
  const integers = [...(audit.targets?.integers ?? [])].map(Number).sort((a, b) => a - b);
  const strings = [...(audit.targets?.strings ?? [])].map((value) => String(value).toLowerCase());
  if (
    audit.schema_version !== 1 ||
    audit.generated_by !== "tools/lua53-constant-audit.py" ||
    JSON.stringify(integers) !== JSON.stringify(TRAINING_SCENE_IDS) ||
    !strings.includes("traininghallid") ||
    Number(audit.summary?.files_scanned) < 4800 ||
    Number(audit.summary?.parse_failures) !== 0
  ) fail("Training-scene Lua constant audit is incomplete or incompatible");

  const trainingHallFiles = luaFilesMatching(audit, (match) =>
    (match.string_hits ?? []).map((value) => String(value).toLowerCase()).includes("traininghallid"));
  if (JSON.stringify(trainingHallFiles) !== JSON.stringify(["dmg_control_view.lua", "Global.lua"])) {
    fail("TrainingHallId appears outside the reviewed global/hidden-control Lua surface");
  }
  return trainingHallFiles;
}

function validateHiddenControlFrontier(frontier) {
  const closure = frontier.proof_closure ?? {};
  if (
    frontier.schema_version !== 1 ||
    frontier.generated_by !== "tools/bpsr-fatal-spiral-controlled-capture-client-frontier.mjs" ||
    frontier.game_build !== GAME_BUILD ||
    closure.exact_build_hidden_damage_control_surface_present !== true ||
    closure.server_gm_command_submission_route_present !== true ||
    closure.shipping_client_blocks_gm_submission !== true ||
    closure.ordinary_production_account_server_authorization_proven !== false ||
    closure.controlled_capture_currently_executable !== false ||
    closure.bypass_of_client_or_server_guards_authorized !== false ||
    closure.provider_rdps_credit_allowed !== false
  ) fail("Hidden controlled-capture frontier is unsafe or incompatible");
}

function semanticDungeonReferences(dungeons, sceneId) {
  const fields = ["SceneID", "SceneId", "scene_id", "sceneId"];
  return Object.entries(dungeons).filter(([key, row]) =>
    Number(key) === sceneId || Number(row?.Id) === sceneId ||
    fields.some((field) => Number(row?.[field]) === sceneId));
}

async function build(options) {
  const files = Object.fromEntries(Object.entries(options).map(([key, value]) =>
    [key, path.resolve(value)]));
  const sceneTable = readJson(files.sceneTable, "SceneTable");
  const dungeonsTable = readJson(files.dungeonsTable, "DungeonsTable");
  const luaAudit = readJson(files.luaAudit, "training-scene Lua audit");
  const hiddenControl = readJson(files.hiddenControlFrontier, "hidden controlled-capture frontier");
  const trainingHallFiles = validateLuaAudit(luaAudit);
  validateHiddenControlFrontier(hiddenControl);

  const scenes = TRAINING_SCENE_IDS.map((sceneId) => {
    const row = sceneTable[String(sceneId)];
    if (
      Number(row?.Id) !== sceneId || Number(row?.SceneType) !== 1 ||
      Number(row?.SceneSubType) !== 4 || !Array.isArray(row?.MapEntryCondition) ||
      row.MapEntryCondition.length !== 0
    ) fail(`SceneTable row ${sceneId} is missing or no longer matches the reviewed practice-scene shape`);
    const dungeonReferences = semanticDungeonReferences(dungeonsTable, sceneId);
    if (dungeonReferences.length !== 0) {
      fail(`Training scene ${sceneId} unexpectedly has a dungeon entry route`);
    }
    return {
      scene_id: sceneId,
      localized_name_evidence: row.Name,
      scene_type: row.SceneType,
      scene_sub_type: row.SceneSubType,
      scene_resource_id: row.SceneResourceId,
      born_id: row.BornId,
      map_entry_conditions: row.MapEntryCondition,
      dungeon_entry_references: 0,
    };
  });

  const [sceneDescriptor, dungeonsDescriptor, auditDescriptor, hiddenDescriptor] = await Promise.all([
    descriptor(files.sceneTable),
    descriptor(files.dungeonsTable),
    descriptor(files.luaAudit),
    descriptor(files.hiddenControlFrontier),
  ]);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game: "blue-protocol-star-resonance",
    game_build: GAME_BUILD,
    identity: {
      training_scene_ids: TRAINING_SCENE_IDS,
      identity_authority: "exact numeric scene ID plus exact client build",
      localized_names_are_evidence_only: true,
    },
    policy: {
      absence_of_a_reviewed_route_is_server_denial_proof: false,
      empty_map_entry_conditions_prove_ordinary_access: false,
      hidden_gm_controls_are_user_authorization: false,
      client_or_server_guards_may_be_bypassed: false,
      organic_capture_may_use_any_observed_scene: true,
      remote_player_cast_packets_required: false,
      missing_remote_packets_are_zero: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      scene_table: sceneDescriptor,
      dungeons_table: dungeonsDescriptor,
      lua_constant_audit: auditDescriptor,
      hidden_controlled_capture_frontier: hiddenDescriptor,
    },
    reviewed_client_evidence: {
      scenes,
      lua_chunks_scanned: luaAudit.summary.files_scanned,
      lua_parse_failures: luaAudit.summary.parse_failures,
      training_hall_identifier_files: trainingHallFiles,
      ordinary_ui_or_service_lua_route_files: [],
      hidden_control_file: "dmg_control_view.lua",
      hidden_submission_route: "world_proxy.GMCommand",
      shipping_client_gm_submission_blocked: true,
    },
    acquisition_decision: {
      scenes_10001_10002_are_currently_executable_capture_routes: false,
      reason:
        "The exact scenes exist, but the reviewed client exposes their TrainingHallId only through a shipping-blocked GM panel and no dungeon or ordinary Lua entry route was found.",
      safe_next_route:
        "Apply the existing same-capture invariant contract to any organic exact-build combat scene; do not require these practice scenes or structurally absent remote casts.",
    },
    proof_closure: {
      exact_build_training_scene_identity_proven: true,
      decoded_dungeon_entry_route_found: false,
      ordinary_ui_or_service_lua_entry_route_found: false,
      ordinary_production_access_proven: false,
      hidden_gm_entry_route_present: true,
      shipping_client_blocks_hidden_route: true,
      authorized_controlled_capture_route_currently_executable: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  verify(report);
  return report;
}

function verify(report) {
  const closure = report.proof_closure ?? {};
  if (
    report.schema_version !== SCHEMA_VERSION || report.generated_by !== GENERATED_BY ||
    report.game_build !== GAME_BUILD ||
    JSON.stringify(report.identity?.training_scene_ids) !== JSON.stringify(TRAINING_SCENE_IDS) ||
    report.policy?.absence_of_a_reviewed_route_is_server_denial_proof !== false ||
    report.policy?.empty_map_entry_conditions_prove_ordinary_access !== false ||
    report.policy?.hidden_gm_controls_are_user_authorization !== false ||
    report.policy?.client_or_server_guards_may_be_bypassed !== false ||
    report.policy?.remote_player_cast_packets_required !== false ||
    report.policy?.missing_remote_packets_are_zero !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    report.reviewed_client_evidence?.lua_parse_failures !== 0 ||
    JSON.stringify(report.reviewed_client_evidence?.training_hall_identifier_files) !==
      JSON.stringify(["dmg_control_view.lua", "Global.lua"]) ||
    report.acquisition_decision?.scenes_10001_10002_are_currently_executable_capture_routes !== false ||
    closure.exact_build_training_scene_identity_proven !== true ||
    closure.decoded_dungeon_entry_route_found !== false ||
    closure.ordinary_ui_or_service_lua_entry_route_found !== false ||
    closure.ordinary_production_access_proven !== false ||
    closure.hidden_gm_entry_route_present !== true ||
    closure.shipping_client_blocks_hidden_route !== true ||
    closure.authorized_controlled_capture_route_currently_executable !== false ||
    closure.formula_authority !== false || closure.runtime_authority !== false ||
    closure.ui_display_authority !== false || closure.provider_rdps_credit_allowed !== false ||
    Number(closure.observed_damage_reassigned_to_provider) !== 0 ||
    report.content_sha256 !== digest(report)
  ) fail("Training-scene access frontier is unsafe or invalid");
}

function parse(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value == null) fail(`Invalid argument ${flag ?? "<missing>"}`);
    args[flag.slice(2)] = value;
  }
  return args;
}

function required(args, name) {
  if (!args[name]) fail(`Missing --${name}`);
  return args[name];
}

function selfTest() {
  const sample = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    identity: { training_scene_ids: TRAINING_SCENE_IDS },
    policy: {
      absence_of_a_reviewed_route_is_server_denial_proof: false,
      empty_map_entry_conditions_prove_ordinary_access: false,
      hidden_gm_controls_are_user_authorization: false,
      client_or_server_guards_may_be_bypassed: false,
      remote_player_cast_packets_required: false,
      missing_remote_packets_are_zero: false,
      provider_rdps_credit_allowed: false,
    },
    reviewed_client_evidence: {
      lua_parse_failures: 0,
      training_hall_identifier_files: ["dmg_control_view.lua", "Global.lua"],
    },
    acquisition_decision: { scenes_10001_10002_are_currently_executable_capture_routes: false },
    proof_closure: {
      exact_build_training_scene_identity_proven: true,
      decoded_dungeon_entry_route_found: false,
      ordinary_ui_or_service_lua_entry_route_found: false,
      ordinary_production_access_proven: false,
      hidden_gm_entry_route_present: true,
      shipping_client_blocks_hidden_route: true,
      authorized_controlled_capture_route_currently_executable: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    content_sha256: "",
  };
  sample.content_sha256 = digest(sample);
  verify(sample);
  sample.proof_closure.ordinary_production_access_proven = true;
  try {
    verify(sample);
    fail("self-test accepted an unproven ordinary training-scene route");
  } catch (error) {
    if (error.message === "self-test accepted an unproven ordinary training-scene route") throw error;
  }
  console.log("bpsr-training-scene-access-frontier self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(path.resolve(required(args, "input")), "training-scene access frontier"));
    console.log("Training-scene access frontier verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = path.resolve(required(args, "output"));
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = await build({
      sceneTable: required(args, "scene-table"),
      dungeonsTable: required(args, "dungeons-table"),
      luaAudit: required(args, "lua-audit"),
      hiddenControlFrontier: required(args, "hidden-control-frontier"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, proof_closure: report.proof_closure }, null, 2));
  } else {
    console.log("Usage:\n  node tools/bpsr-training-scene-access-frontier.mjs build --scene-table <json> --dungeons-table <json> --lua-audit <json> --hidden-control-frontier <json> --output <json>\n  node tools/bpsr-training-scene-access-frontier.mjs verify --input <json>\n  node tools/bpsr-training-scene-access-frontier.mjs self-test");
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
