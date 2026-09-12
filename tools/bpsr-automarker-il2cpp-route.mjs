#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const BUILD = "25247556";
const APP_ID = "3681810";
const GAME_ASSEMBLY_SHA256 = "4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3";
const PROCESS_EXECUTABLE_SHA256 = "90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588";
const METADATA_MAGIC = 0xfab11baf;

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function artifact(path, bytes) {
  return { path: resolve(path), byte_length: bytes.length, sha256: sha256(bytes) };
}

function sameArtifact(receipt, observed, metadataVersion = undefined) {
  return Number(receipt?.byte_length) === observed.byte_length &&
    same(String(receipt?.sha256 ?? ""), observed.sha256) &&
    (metadataVersion === undefined || Number(receipt?.metadata_version) === metadataVersion);
}

export function validateRecoveryIdentity(identity, metadata, assembly, metadataVersion) {
  if (identity?.schema_version !== 2 ||
      identity?.generated_by !== "rlogs-bpsr-il2cpp-metadata-scan" ||
      identity?.game !== "blue-protocol-star-resonance" ||
      identity?.deployment !== "global" || identity?.channel !== "steam" ||
      String(identity?.game_build) !== BUILD || String(identity?.distribution_app_id) !== APP_ID ||
      !sameArtifact(identity?.metadata, metadata, metadataVersion) ||
      !sameArtifact(identity?.game_assembly, assembly) ||
      !same(String(identity?.game_assembly?.sha256 ?? ""), GAME_ASSEMBLY_SHA256) ||
      !same(String(identity?.process_executable?.sha256 ?? ""), PROCESS_EXECUTABLE_SHA256) ||
      Number(identity?.process_executable?.byte_length) !== 808496 ||
      Number(identity?.steam_manifest?.byte_length) <= 0 ||
      !/^[0-9a-f]{64}$/i.test(String(identity?.steam_manifest?.sha256 ?? ""))) {
    throw new Error("identity receipt does not bind the exact recovered build-25247556 inputs");
  }
}

function parsePeImageBase(bytes) {
  if (bytes.length < 0x100 || bytes.readUInt16LE(0) !== 0x5a4d) throw new Error("GameAssembly is not a PE image");
  const pe = bytes.readUInt32LE(0x3c);
  if (pe + 0x38 > bytes.length || bytes.toString("ascii", pe, pe + 4) !== "PE\0\0") throw new Error("GameAssembly has no valid PE signature");
  const optional = pe + 24;
  if (bytes.readUInt16LE(optional) !== 0x20b) throw new Error("GameAssembly is not PE32+");
  return bytes.readBigUInt64LE(optional + 24);
}

export function parseDump(text) {
  const methods = [];
  let namespace = "";
  let type = null;
  let pending = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    const ns = line.match(/^\/\/ Namespace:\s*(.*)$/);
    if (ns) { namespace = ns[1].trim(); type = null; continue; }
    const declared = line.match(/^(?:(?:public|private|protected|internal|static|abstract|sealed|partial|readonly)\s+)*(?:class|struct|interface)\s+([^\s:{]+)(?:[^/]*?\/\/ TypeDefIndex:\s*(\d+))?/);
    if (declared) {
      type = { name: declared[1], type_def_index: declared[2] === undefined ? null : Number(declared[2]) };
      continue;
    }
    const address = line.match(/^\/\/ RVA:\s*(0x[0-9a-f]+|-1)\s+Offset:\s*(0x[0-9a-f]+|-1)(?:\s+VA:\s*(0x[0-9a-f]+|-1))?/i);
    if (address) {
      pending = { rva: address[1], offset: address[2], va: address[3] ?? null };
      continue;
    }
    if (!pending || !type || line.startsWith("//") || !line.includes("(") || !line.includes(")")) continue;
    const name = line.slice(0, line.indexOf("(")).trim().split(/\s+/).at(-1);
    if (!name) { pending = null; continue; }
    methods.push({
      namespace,
      type: type.name,
      type_def_index: type.type_def_index,
      method: name,
      signature: line.replace(/\s*\{\s*\}\s*$/, "").replace(/;$/, ""),
      rva: pending.rva === "-1" ? null : pending.rva.toUpperCase().replace("0X", "0x"),
      offset: pending.offset === "-1" ? null : pending.offset.toUpperCase().replace("0X", "0x"),
      dump_va: !pending.va || pending.va === "-1" ? null : pending.va.toUpperCase().replace("0X", "0x"),
    });
    pending = null;
  }
  return methods;
}

function same(value, expected) { return value.toLocaleLowerCase("en-US") === expected.toLocaleLowerCase("en-US"); }
function contains(value, fragment) { return value.toLocaleLowerCase("en-US").includes(fragment.toLocaleLowerCase("en-US")); }

export function routeMethods(methods, imageBase = 0x180000000n) {
  const native = (rows) => rows.map((row) => {
    if (!row.rva) return { ...row, computed_va: null, dump_va_matches_image: null };
    const computed = `0x${(imageBase + BigInt(row.rva)).toString(16).toUpperCase()}`;
    return { ...row, computed_va: computed, dump_va_matches_image: row.dump_va === null ? null : same(row.dump_va, computed) };
  });
  const flagSkill = native(methods.filter((m) => same(m.type, "PlayerInputController") && same(m.method, "FlagSkill")));
  const trySetAxis = native(methods.filter((m) => same(m.method, "TrySetAxis") &&
    (same(m.type, "CustomPadDevice") || same(m.type, "TouchController") || contains(m.type, "Touch"))));
  const useSlot = native(methods.filter((m) =>
    (same(m.method, "UseSlot") && contains(m.type, "World")) ||
    (same(m.method, "MoveNext") && /<UseSlot>d__\d+/i.test(m.type))));
  const setSelectPoint = native(methods.filter((m) =>
    same(m.type, "ZBattleUtils") && same(m.method, "SetSelectPoint")));
  const setIndicatorPos = native(methods.filter((m) =>
    same(m.type, "EntityAttrExtensions") && same(m.method, "SetIndicatorPos") &&
    contains(m.namespace, "ZGame")));
  const firePlaySkillByIndicator = native(methods.filter((m) =>
    same(m.type, "ZSkillInputMgr") && same(m.method, "FirePlaySkillByIndicator")));
  const targeting = native(methods.filter((m) => {
    const identity = `${m.namespace}.${m.type}.${m.method}`;
    return /(raycast|ground.*target|target.*ground|skill.*target|target.*position|aim.*point|place.*position)/i.test(identity);
  }));
  return {
    exact: {
      player_input_controller_flag_skill: flagSkill,
      axis_input_try_set_axis: trySetAxis,
      world_use_slot_bridge: useSlot,
      z_battle_utils_set_select_point: setSelectPoint,
      entity_attr_extensions_set_indicator_pos: setIndicatorPos,
      z_skill_input_mgr_fire_play_skill_by_indicator: firePlaySkillByIndicator,
    },
    discovery: { target_or_raycast_candidates: targeting },
    gates: {
      flag_skill_unique_native: flagSkill.length === 1 && flagSkill[0].rva !== null,
      try_set_axis_unique_native: trySetAxis.length === 1 && trySetAxis[0].rva !== null,
      use_slot_bridge_present: useSlot.some((m) => m.rva !== null),
      exact_position_primitives_present:
        setSelectPoint.length === 1 && setSelectPoint[0].rva !== null &&
        setIndicatorPos.length === 1 && setIndicatorPos[0].rva !== null &&
        firePlaySkillByIndicator.length === 1 && firePlaySkillByIndicator[0].rva !== null,
      target_or_raycast_handler_proven: false,
      executable_native_activation_proven: false,
    },
  };
}

function parseArgs(argv) {
  const result = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i].startsWith("--") || argv[i + 1] === undefined) throw new Error(`invalid argument ${argv[i] ?? ""}`);
    result[argv[i].slice(2)] = argv[i + 1];
  }
  return result;
}

function selfTest() {
  const fixture = `// Namespace: Game.Input\npublic class PlayerInputController // TypeDefIndex: 10\n{\n// Methods\n// RVA: 0x1234 Offset: 0x1034 VA: 0x180001234\npublic void FlagSkill(int slot, bool pressed) { }\n}\n// Namespace: Game.Input\ninternal class CustomPadDevice // TypeDefIndex: 11\n{\n// RVA: 0x2345 Offset: 0x2045 VA: 0x180002345\npublic bool TrySetAxis(int elementId, float value) { }\n}\n// Namespace: Zservice\nprivate struct WorldProxy.<UseSlot>d__19 // TypeDefIndex: 12\n{\n// RVA: 0x3456 Offset: 0x3056 VA: 0x180003456\nprivate void MoveNext() { }\n}\n// Namespace: Panda.ZGame\npublic static class ZBattleUtils // TypeDefIndex: 13\n{\n// RVA: 0x4567 Offset: 0x4057 VA: 0x180004567\npublic static void SetSelectPoint(ZEntity host, ESelectPosSource source, ESkillTargetRangeType rangeType, ESkillSelectPointType selectPointType, int skillEffectId, Quaternion targetRot, Vector3 forcePos, bool isIndicator = False) { }\n}\n// Namespace: Panda.ZGame\npublic static class EntityAttrExtensions // TypeDefIndex: 14\n{\n// RVA: 0x5678 Offset: 0x5058 VA: 0x180005678\npublic static void SetIndicatorPos(ZEntity entity, Vector3 position) { }\n}\n// Namespace: Panda.ZGame\ninternal class ZSkillInputMgr // TypeDefIndex: 15\n{\n// RVA: 0x6789 Offset: 0x6069 VA: 0x180006789\npublic bool FirePlaySkillByIndicator(int skillID) { }\n}\n// Namespace: Game.Targeting\npublic class GroundRaycastController // TypeDefIndex: 16\n{\n// RVA: 0x789A Offset: 0x707A VA: 0x18000789A\nprivate bool RaycastGroundPosition() { }\n}`;
  const methods = parseDump(fixture);
  const route = routeMethods(methods);
  if (methods.length !== 7 || !route.gates.flag_skill_unique_native || !route.gates.try_set_axis_unique_native || !route.gates.use_slot_bridge_present || !route.gates.exact_position_primitives_present) throw new Error("self-test exact routing failed");
  if (route.discovery.target_or_raycast_candidates.length !== 1 || route.gates.target_or_raycast_handler_proven) throw new Error("self-test discovery gate failed");
  if (route.gates.executable_native_activation_proven) throw new Error("self-test opened native activation without runtime proof");
  const metadata = { byte_length: 1234, sha256: "a".repeat(64) };
  const assembly = { byte_length: 218074672, sha256: GAME_ASSEMBLY_SHA256 };
  const identity = {
    schema_version: 2, generated_by: "rlogs-bpsr-il2cpp-metadata-scan",
    game: "blue-protocol-star-resonance", deployment: "global", channel: "steam",
    game_build: BUILD, distribution_app_id: APP_ID,
    metadata: { ...metadata, metadata_version: 31 }, game_assembly: assembly,
    process_executable: { byte_length: 808496, sha256: PROCESS_EXECUTABLE_SHA256 },
    steam_manifest: { byte_length: 100, sha256: "b".repeat(64) },
  };
  validateRecoveryIdentity(identity, metadata, assembly, 31);
  identity.metadata.sha256 = "c".repeat(64);
  try { validateRecoveryIdentity(identity, metadata, assembly, 31); throw new Error("self-test accepted mismatched metadata identity"); }
  catch (error) { if (!error.message.includes("does not bind")) throw error; }
  process.stdout.write("BPSR automarker IL2CPP route self-test passed.\n");
}

function main() {
  if (process.argv[2] === "self-test") return selfTest();
  if (process.argv[2] !== "build") throw new Error("Usage: node tools/bpsr-automarker-il2cpp-route.mjs build --build 25247556 --metadata <global-metadata.dat> --identity <client-binary-identity.json> --game-assembly <GameAssembly.dll> --dump <dump.cs> --output <json>");
  const args = parseArgs(process.argv.slice(3));
  for (const key of ["build", "metadata", "identity", "game-assembly", "dump", "output"]) if (!args[key]) throw new Error(`--${key} is required`);
  if (args.build !== BUILD) throw new Error(`this router is locked to build ${BUILD}`);
  const metadata = readFileSync(args.metadata);
  const identityBytes = readFileSync(args.identity);
  const assembly = readFileSync(args["game-assembly"]);
  const dump = readFileSync(args.dump);
  if (sha256(assembly) !== GAME_ASSEMBLY_SHA256) throw new Error(`GameAssembly SHA-256 does not match exact build ${BUILD}`);
  if (metadata.length < 0x180 || metadata.readUInt32LE(0) !== METADATA_MAGIC) throw new Error("metadata is empty or lacks the IL2CPP metadata magic/header");
  const metadataVersion = metadata.readInt32LE(4);
  if (metadataVersion < 20 || metadataVersion > 99) throw new Error(`implausible IL2CPP metadata version ${metadataVersion}`);
  const metadataArtifact = artifact(args.metadata, metadata);
  const assemblyArtifact = artifact(args["game-assembly"], assembly);
  let recoveryIdentity;
  try { recoveryIdentity = JSON.parse(identityBytes.toString("utf8")); }
  catch { throw new Error("identity receipt is not valid JSON"); }
  validateRecoveryIdentity(recoveryIdentity, metadataArtifact, assemblyArtifact, metadataVersion);
  const imageBase = parsePeImageBase(assembly);
  const methods = parseDump(dump.toString("utf8"));
  if (methods.length === 0) throw new Error("dump.cs contains no address-bearing method declarations");
  const routes = routeMethods(methods, imageBase);
  const mismatchedVas = Object.values(routes.exact).flat().concat(routes.discovery.target_or_raycast_candidates).filter((m) => m.dump_va_matches_image === false);
  if (mismatchedVas.length) throw new Error("dump.cs VA values do not match the exact GameAssembly image base");
  const result = {
    schema_version: 1,
    generated_by: "tools/bpsr-automarker-il2cpp-route.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    build_id: BUILD,
    scope: { offline_only: true, process_access: false, game_modification: false, network_transmission: false },
    inputs: { metadata: { ...metadataArtifact, metadata_version: metadataVersion }, recovery_identity: artifact(args.identity, identityBytes), game_assembly: assemblyArtifact, il2cpp_dump: artifact(args.dump, dump) },
    image_base: `0x${imageBase.toString(16).toUpperCase()}`,
    parsed_address_bearing_methods: methods.length,
    routes,
    conclusion: {
      exact_named_entry_points_resolved: routes.gates.flag_skill_unique_native && routes.gates.try_set_axis_unique_native && routes.gates.use_slot_bridge_present,
      target_or_raycast_handler_resolved: false,
      exact_position_primitives_resolved: routes.gates.exact_position_primitives_present,
      executable_native_activation_enabled: false,
      note: "Name matches are routing evidence only. The exact-position primitives do not prove safe object acquisition or a FlagSkill-compatible activation sequence; native activation remains closed.",
    },
  };
  writeFileSync(args.output, `${JSON.stringify(result, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`wrote ${resolve(args.output)}\n`);
}

try { main(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
