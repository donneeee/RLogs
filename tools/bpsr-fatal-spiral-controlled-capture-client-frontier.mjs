#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-fatal-spiral-controlled-capture-client-frontier.mjs";
const GAME_BUILD = "24687926";
const GAME_ASSEMBLY_SHA256 =
  "4ba9e3f194bfd1769e57e3f12d192208e4d34db04374636738dfc9d5525495a4";
const BLOCK_GM_GETTER_RVA = 0x45df690;
const BLOCK_GM_TRUE_EPILOGUE = Buffer.from("b0014883c428c3", "hex");

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

function decompile(decompiler, input, label) {
  const result = spawnSync(decompiler, ["--dec", input], {
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.error || result.status !== 0) {
    fail(`Cannot decompile ${label}: ${result.error?.message ?? result.stderr}`);
  }
  return result.stdout.replaceAll(/\s+/g, " ");
}

function requireFragments(source, fragments, label) {
  for (const fragment of fragments) {
    if (!source.includes(fragment)) fail(`${label} is missing ${JSON.stringify(fragment)}`);
  }
}

function readPeBytesAtRva(file, rva, length) {
  const handle = fs.openSync(file, "r");
  try {
    const dos = Buffer.alloc(64);
    fs.readSync(handle, dos, 0, dos.length, 0);
    if (dos.toString("ascii", 0, 2) !== "MZ") fail("GameAssembly is not a PE image");
    const peOffset = dos.readUInt32LE(0x3c);
    const coff = Buffer.alloc(24);
    fs.readSync(handle, coff, 0, coff.length, peOffset);
    if (coff.toString("ascii", 0, 4) !== "PE\0\0") fail("GameAssembly PE signature is invalid");
    const sectionCount = coff.readUInt16LE(6);
    const optionalHeaderBytes = coff.readUInt16LE(20);
    const sectionTableOffset = peOffset + 24 + optionalHeaderBytes;
    const sections = Buffer.alloc(sectionCount * 40);
    fs.readSync(handle, sections, 0, sections.length, sectionTableOffset);
    for (let index = 0; index < sectionCount; index += 1) {
      const offset = index * 40;
      const virtualSize = sections.readUInt32LE(offset + 8);
      const virtualAddress = sections.readUInt32LE(offset + 12);
      const rawSize = sections.readUInt32LE(offset + 16);
      const rawOffset = sections.readUInt32LE(offset + 20);
      const span = Math.max(virtualSize, rawSize);
      if (rva >= virtualAddress && rva + length <= virtualAddress + span) {
        const bytes = Buffer.alloc(length);
        fs.readSync(handle, bytes, 0, length, rawOffset + rva - virtualAddress);
        return bytes;
      }
    }
    fail(`RVA 0x${rva.toString(16)} is outside every PE section`);
  } finally {
    fs.closeSync(handle);
  }
}

async function build(options) {
  const files = Object.fromEntries(Object.entries(options).map(([key, value]) =>
    [key, path.resolve(value)]));
  const identity = readJson(files.clientIdentity, "client binary identity");
  if (
    String(identity.game_build) !== GAME_BUILD ||
    identity.game_assembly?.sha256 !== GAME_ASSEMBLY_SHA256
  ) fail("Client binary identity is not the reviewed exact build");

  const [
    identityDescriptor,
    assemblyDescriptor,
    decompilerDescriptor,
    controlDescriptor,
    damageVmDescriptor,
    gmVmDescriptor,
    defineDescriptor,
  ] = await Promise.all([
    descriptor(files.clientIdentity),
    descriptor(files.gameAssembly),
    descriptor(files.decompiler),
    descriptor(files.damageControlView),
    descriptor(files.damageVm),
    descriptor(files.gmVm),
    descriptor(files.defineLua),
  ]);
  if (assemblyDescriptor.sha256 !== GAME_ASSEMBLY_SHA256) {
    fail("GameAssembly hash does not match the reviewed exact build");
  }

  const control = decompile(files.decompiler, files.damageControlView, "damage-control view");
  const damageVm = decompile(files.decompiler, files.damageVm, "damage VM");
  const gmVm = decompile(files.decompiler, files.gmVm, "GM VM");
  const defineLua = decompile(files.decompiler, files.defineLua, "Lua definitions");
  requireFragments(control, [
    'string.zconcat("addBuff ", dmgData.ControlBuffId',
    'string.zconcat("delBuff ", dmgData.ControlBuffId',
    'gmVm.SubmitGmCmd("clearGMAttr", self.cancelSource)',
    'string.zconcat("addGMAttr ", id, "|", dmgData.ControlAttrCount',
    'string.zconcat("monsterForceUseSkill ", dmgData.ControlNowSelectTargetUuid',
    'string.zconcat("enterScene ", trainingHallIdTab[1][1])',
  ], "damage-control view");
  requireFragments(damageVm, [
    'OpenDamageView = function() Z.UIMgr:OpenView("dmg_control")',
    'Z.UIMgr:OpenView("dmg_data_panel")',
  ], "damage VM");
  requireFragments(gmVm, [
    'function GmVM.SubmitGmCmd(cmdStr, cancelSource, targetId) if Z.IsBlockGM then return end',
    'local gmProxy = require("zproxy.world_proxy")',
    'local ret = gmProxy.GMCommand(cmd, cancelToken)',
  ], "GM VM");
  requireFragments(defineLua, [
    'Z.IsBlockGM = Panda.Core.Wrap.GameContext.IsBlockGM',
  ], "Lua definitions");

  const getterBytes = readPeBytesAtRva(files.gameAssembly, BLOCK_GM_GETTER_RVA, 80);
  const trueEpilogueOffset = getterBytes.indexOf(BLOCK_GM_TRUE_EPILOGUE);
  if (trueEpilogueOffset < 0) fail("Exact-build IsBlockGM getter does not return true");

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game: "blue-protocol-star-resonance",
    game_build: GAME_BUILD,
    identity: {
      effect_id: 2110125,
      provider_marker_effect_id: 2110124,
      all_element_attribute_ids: [13100, 13101, 13102, 13103, 13104, 13105],
      game_assembly_sha256: GAME_ASSEMBLY_SHA256,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      hidden_client_controls_are_user_authorization: false,
      blocked_gm_routes_may_be_bypassed: false,
      server_acceptance_is_assumed: false,
      client_control_surface_is_formula_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      client_binary_identity: identityDescriptor,
      game_assembly: assemblyDescriptor,
      lua_decompiler: decompilerDescriptor,
      damage_control_view_bytecode: controlDescriptor,
      damage_vm_bytecode: damageVmDescriptor,
      gm_vm_bytecode: gmVmDescriptor,
      lua_definitions_bytecode: defineDescriptor,
    },
    reviewed_client_surface: {
      damage_control_view: "ui/view/dmg/dmg_control_view.lua",
      exact_server_command_templates: [
        "addBuff",
        "delBuff",
        "addGMAttr",
        "clearGMAttr",
        "monsterForceUseSkill",
        "enterScene",
      ],
      selectable_control_axes: [
        "exact target entity",
        "exact buff id, count, duration, and parameter",
        "exact fight attribute id and value",
        "exact monster or dummy skill id",
        "training-hall scene",
      ],
      submission_route: "zproxy.world_proxy.GMCommand",
      client_gate: {
        lua_binding: "Panda.Core.Wrap.GameContext.IsBlockGM",
        getter_rva: `0x${BLOCK_GM_GETTER_RVA.toString(16).toUpperCase()}`,
        inspected_bytes: getterBytes.toString("hex"),
        true_epilogue_offset: trueEpilogueOffset,
        exact_build_returns_true: true,
      },
    },
    proof_closure: {
      exact_build_hidden_damage_control_surface_present: true,
      exact_buff_attribute_target_and_skill_controls_present: true,
      server_gm_command_submission_route_present: true,
      shipping_client_blocks_gm_submission: true,
      ordinary_production_account_server_authorization_proven: false,
      controlled_capture_currently_executable: false,
      bypass_of_client_or_server_guards_authorized: false,
      current_controlled_pairs_available: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    next_acquisition: {
      authorized_internal_or_qa_session:
        "run the exact damage-control surface on a server/account explicitly authorized for GM commands",
      ordinary_player_session:
        "capture a same-build repeated damage action before and during an organically applied effect 2110125 while holding every other observed input invariant",
      remote_player_cast_packet_required: false,
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
    report.schema_version !== SCHEMA_VERSION ||
    report.generated_by !== GENERATED_BY ||
    report.game_build !== GAME_BUILD ||
    Number(report.identity?.effect_id) !== 2110125 ||
    report.identity?.game_assembly_sha256 !== GAME_ASSEMBLY_SHA256 ||
    report.policy?.hidden_client_controls_are_user_authorization !== false ||
    report.policy?.blocked_gm_routes_may_be_bypassed !== false ||
    report.policy?.server_acceptance_is_assumed !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    closure.exact_build_hidden_damage_control_surface_present !== true ||
    closure.exact_buff_attribute_target_and_skill_controls_present !== true ||
    closure.server_gm_command_submission_route_present !== true ||
    closure.shipping_client_blocks_gm_submission !== true ||
    closure.ordinary_production_account_server_authorization_proven !== false ||
    closure.controlled_capture_currently_executable !== false ||
    closure.bypass_of_client_or_server_guards_authorized !== false ||
    closure.current_controlled_pairs_available !== false ||
    closure.formula_authority !== false ||
    closure.runtime_authority !== false ||
    closure.ui_display_authority !== false ||
    closure.provider_rdps_credit_allowed !== false ||
    Number(closure.observed_damage_reassigned_to_provider) !== 0 ||
    report.content_sha256 !== digest(report)
  ) fail("Fatal Spiral controlled-capture client frontier is unsafe or invalid");
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
    identity: { effect_id: 2110125, game_assembly_sha256: GAME_ASSEMBLY_SHA256 },
    policy: {
      hidden_client_controls_are_user_authorization: false,
      blocked_gm_routes_may_be_bypassed: false,
      server_acceptance_is_assumed: false,
      provider_rdps_credit_allowed: false,
    },
    proof_closure: {
      exact_build_hidden_damage_control_surface_present: true,
      exact_buff_attribute_target_and_skill_controls_present: true,
      server_gm_command_submission_route_present: true,
      shipping_client_blocks_gm_submission: true,
      ordinary_production_account_server_authorization_proven: false,
      controlled_capture_currently_executable: false,
      bypass_of_client_or_server_guards_authorized: false,
      current_controlled_pairs_available: false,
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
  sample.proof_closure.controlled_capture_currently_executable = true;
  try {
    verify(sample);
    fail("self-test accepted an unauthorized controlled-capture route");
  } catch (error) {
    if (error.message === "self-test accepted an unauthorized controlled-capture route") throw error;
  }
  console.log("bpsr-fatal-spiral-controlled-capture-client-frontier self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(path.resolve(required(args, "input")), "controlled-capture frontier"));
    console.log("Fatal Spiral controlled-capture client frontier verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = path.resolve(required(args, "output"));
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = await build({
      clientIdentity: required(args, "client-identity"),
      gameAssembly: required(args, "game-assembly"),
      decompiler: required(args, "decompiler"),
      damageControlView: required(args, "damage-control-view"),
      damageVm: required(args, "damage-vm"),
      gmVm: required(args, "gm-vm"),
      defineLua: required(args, "define-lua"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, proof_closure: report.proof_closure }, null, 2));
  } else {
    console.log("Usage:\n  node tools/bpsr-fatal-spiral-controlled-capture-client-frontier.mjs build --client-identity <json> --game-assembly <dll> --decompiler <exe> --damage-control-view <lua> --damage-vm <lua> --gm-vm <lua> --define-lua <lua> --output <json>\n  node tools/bpsr-fatal-spiral-controlled-capture-client-frontier.mjs verify --input <json>\n  node tools/bpsr-fatal-spiral-controlled-capture-client-frontier.mjs self-test");
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
