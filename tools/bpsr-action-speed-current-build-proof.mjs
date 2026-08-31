#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const EXPECTED_BUILD = "24687926";
const EXPECTED_DEPLOYMENT = "global";
const SCHEMA_VERSION = 5;
const EXPECTED_BINARY_BYTES = 217_629_232;
const EXPECTED_BINARY_SHA256 = "4ba9e3f194bfd1769e57e3f12d192208e4d34db04374636738dfc9d5525495a4";
const FIXED_POINT_SCALE_RVA = 0x8c4ee0c;
const ONE_FLOAT_RVA = 0x8c4df04;

const METHODS = [
  {
    name: "Panda.ZGame.ZBattleUtils.GetSkillSpeed",
    rva: 0x5294520,
    end_rva: 0x5294c40,
    sha256: "a6f341219cae177b1e987f6906c55ce43c653e46e3b4366458e887ca940d67a7",
  },
  {
    name: "Panda.ZGame.ZBattleUtils.getSkillSpeed",
    rva: 0x5294c40,
    end_rva: 0x5294d30,
    sha256: "6b5352d1bd4eeacf1d71f95ebc8bbb92956f6ec640085c5aa3dd4888dfb5a36e",
  },
  {
    name: "Panda.ZGame.ZBattleUtils.GetSkillReduce",
    rva: 0x5294d30,
    end_rva: 0x5294dd0,
    sha256: "f192fd48c6b9d1094512b8435ea499721ab07a2e78aa0eb0c5b39773f4b51a57",
  },
  {
    name: "Panda.ZGame.ZBattleUtils.getSkillReduce",
    rva: 0x5294dd0,
    end_rva: 0x52951b0,
    sha256: "1bb7467c54c26e8eabb083493a9dd7327dd7942e2a9fcc3a6866acff55ec03dc",
  },
  {
    name: "TempAttrComp.TryGetTempAttrByType (native ABI; conflicting dump label)",
    rva: 0x4140810,
    end_rva: 0x4140bd0,
    sha256: "352d22f7ec2d64bd4ad2e7544e9a90a2349e2d6416168eac99d4b8f5af7418d1",
  },
  {
    name: "Panda.ZGame.ZBattleUtils singing-speed wrapper (native ABI; conflicting dump label)",
    rva: 0x52951b0,
    end_rva: 0x5295240,
    sha256: "a7b85cb8b8da550f595eeca9c56b400f4e4d8108e4e7ef4ab4d1a59aa42bf019",
  },
  {
    name: "Panda.ZGame.ZBattleUtils singing-speed body (native ABI; conflicting dump label)",
    rva: 0x5295240,
    end_rva: 0x5295470,
    sha256: "62d1c0eb3bcdd92e121d139944e8500a24eba4e866894699dc249ffaf2059fe2",
  },
];

const DUMP_METHODS = [
  ["GetSkillSpeed", 0x5294520, "public static float GetSkillSpeed(ZEntity entity, int skillId, StageLogicInfo stageLogicInfo)"],
  ["getSkillSpeed", 0x5294c40, "private static float getSkillSpeed(ZEntity entity, int skillId, EStageType stageType)"],
  ["GetSkillReduce", 0x5294d30, "public static float GetSkillReduce(ZEntity entity, int skillId, EStageType stageType)"],
  ["getSkillReduce", 0x5294dd0, "private static float getSkillReduce(ZEntity entity, int skillId, EStageType stageType)"],
  ["AtkSpeedSwitch", 0x5657c90, "public bool get_AtkSpeedSwitch()"],
];

const CONFLICTING_DUMP_LABELS = [
  ["GetStageType", 0x52951b0, "public static EStageType GetStageType(long skillLevelId, int stageId)"],
  ["StopPlayerAIBattle", 0x5295240, "public static void StopPlayerAIBattle()"],
  ["TryGetTempAttrById", 0x4140810, "public bool TryGetTempAttrById(long tempAttrConfigId, out int value)"],
];

const REVIEWED_ANCHORS = [
  {
    id: "entity-type-8-selects-pet-lane",
    rva: 0x5294f52,
    bytes: "83f8080f8414020000",
    interpretation: "entity type 8 branches to the pet speed lane before ordinary stage selection",
  },
  {
    id: "singing-reduce-helper-call-abi",
    rva: 0x5295063,
    bytes: "4533c941b8010000008bd6488bcfe83a010000",
    interpretation: "caller passes entity, skill ID, and stage type 1 to RVA 0x52951b0 and consumes a float result",
    call_offset: 14,
    call_target_rva: 0x52951b0,
  },
  {
    id: "mislabelled-wrapper-tail-jump",
    rva: 0x529521c,
    bytes: "e91f000000",
    interpretation: "RVA 0x52951b0 tail-jumps to the native singing-speed body at RVA 0x5295240, contradicting both dump labels",
    call_offset: 0,
    call_target_rva: 0x5295240,
  },
  {
    id: "stage-family-selector",
    rva: 0x5294f5b,
    bytes: "8bcd85ed0f843901000083e9010f84c700000083f90174698d45f883f8010f878a000000",
    interpretation: "stage 0 selects normal, stage 1 singing, stage 2 charge, stages 8 and 9 guide, and all other values remain unaffected",
  },
  {
    id: "guide-cast-speed-attribute",
    rva: 0x5294f89,
    bytes: "bad22d0000",
    interpretation: "guide stages read exact numeric attribute 11730",
    numeric_id: 11730,
  },
  {
    id: "guide-cast-speed-temporary-effect",
    rva: 0x5294fd2,
    bytes: "bac6020000e93b010000",
    interpretation: "guide stages load temporary effect type 710 and jump to the shared skill-scoped lookup ABI immediately after the normal lane's effect-type load",
    numeric_id: 710,
    relative_offset: 5,
    relative_target_rva: 0x5295117,
  },
  {
    id: "charge-speed-attribute",
    rva: 0x5294fe6,
    bytes: "badc2d0000",
    interpretation: "charge stages read exact numeric attribute 11740",
    numeric_id: 11740,
  },
  {
    id: "normal-attack-speed-switch-call",
    rva: 0x52950ae,
    bytes: "e8dd2b3c00",
    interpretation: "normal-stage speed is conditional on SkillTable.AtkSpeedSwitch",
    call_target_rva: 0x5657c90,
  },
  {
    id: "normal-attack-speed-attribute",
    rva: 0x52950c5,
    bytes: "bac82d0000",
    interpretation: "enabled normal stages read exact numeric attribute 11720",
    numeric_id: 11720,
  },
  {
    id: "normal-attack-speed-temporary-effect",
    rva: 0x5295112,
    bytes: "babc020000",
    interpretation: "enabled normal stages add skill-scoped temporary attribute effect type 700",
    numeric_id: 700,
  },
  {
    id: "temporary-attribute-lookup-call-abi",
    rva: 0x5295112,
    bytes: "babc020000488d8c24900000004c897c242848894c2420448bce488bc84489bc249000000041b801000000e8ceb6eafe",
    interpretation: "the normal lane passes effect type 700 in RDX, TempAttrSkill 1 in R8D, skill ID in R9D, an out-value pointer as the fifth argument, and the temporary-attribute component in RCX before calling RVA 0x4140810",
    numeric_id: 700,
    logic_type: 1,
    parameter: "skill_id",
    out_value_argument: 5,
    call_offset: 43,
    call_target_rva: 0x4140810,
  },
  {
    id: "pet-attack-speed-attribute",
    rva: 0x5295179,
    bytes: "bad62e0000",
    interpretation: "pet stages read exact numeric attribute 11990",
    numeric_id: 11990,
  },
  {
    id: "temporary-lookup-native-argument-capture",
    rva: 0x4140832,
    bytes: "458be9458be0448bfa488bf9",
    interpretation: "the lookup body preserves R9D as the skill parameter, R8D as logic type, EDX as effect type, and RCX as the temporary-attribute component",
  },
  {
    id: "temporary-lookup-output-initialized-zero",
    rva: 0x41408bd,
    bytes: "488bb42400010000c70600000000",
    interpretation: "the fifth-argument output pointer is loaded and initialized to signed integer zero before dictionary lookup",
  },
  {
    id: "temporary-lookup-effect-type-dictionary-key",
    rva: 0x41408ef,
    bytes: "4d8b46384d8b4010418bd7488bcfe85ee1fbfc85c00f887a",
    interpretation: "the outer temporary-attribute dictionary is queried with the preserved exact effect type",
  },
  {
    id: "temporary-lookup-logic-type-dictionary-key",
    rva: 0x41409b7,
    bytes: "4d8b46384d8b4010418bd4488bcfe896e0fbfc85c00f88b2",
    interpretation: "the nested temporary-attribute dictionary is queried with the preserved exact logic type",
  },
  {
    id: "temporary-lookup-skill-filter-and-signed-sum",
    rva: 0x4140b2d,
    bytes: "4585e4742d660f6fc6660f73d80866480f7ec14885c9746a4c8b01498b80d80100004d8b80e0010000418bd5ffd084c074b1660f7ef00106b301",
    interpretation: "non-global logic types invoke the entry predicate with the preserved skill parameter; every matching signed integer value is added to the output and marks the lookup successful",
  },
  {
    id: "temporary-lookup-no-match-return",
    rva: 0x4140b70,
    bytes: "eb12488b4c24284885c975390fb69c24e80000000fb6c3",
    interpretation: "enumeration completion returns the match flag while the preinitialized output remains zero when no entry matched",
  },
  {
    id: "guide-attribute-float32-operation-order",
    rva: 0x5294fa3,
    bytes: "660f6ec00f5bc0f30f5ec7f30f58c60f28f0",
    interpretation: "guide converts signed attribute i32 to float32, divides by float32 10000, then adds float32 1",
  },
  {
    id: "charge-attribute-float32-operation-order",
    rva: 0x5294ff3,
    bytes: "660f6ec00f5bc0f30f5e050a9e9b03f30f58c60f28f0",
    interpretation: "charge converts signed attribute i32 to float32, divides by float32 10000, then adds float32 1",
  },
  {
    id: "normal-attribute-float32-operation-order",
    rva: 0x52950df,
    bytes: "660f6ec00f5bc0f30f5ec7f30f58c60f28f0",
    interpretation: "enabled normal converts signed attribute i32 to float32, divides by float32 10000, then adds float32 1",
  },
  {
    id: "normal-guide-temporary-float32-operation-order",
    rva: 0x5295156,
    bytes: "660f6e8424900000000f5bc0f30f5ec7f30f58f0",
    interpretation: "a matching normal or guide temporary signed i32 is converted to float32, divided by float32 10000, then added to the prior float32 speed",
  },
  {
    id: "pet-attribute-float32-operation-order",
    rva: 0x5295186,
    bytes: "660f6ec00f5bc0f30f5e05779c9b03f30f58c6",
    interpretation: "pet converts signed attribute i32 to float32, divides by float32 10000, then adds float32 1",
  },
  {
    id: "singing-float32-one-initialization",
    rva: 0x5295381,
    bytes: "f30f10357b8b9b03",
    interpretation: "the singing-speed body initializes its accumulator from the exact float32 1 constant",
  },
  {
    id: "singing-attribute-float32-operation-order",
    rva: 0x52953af,
    bytes: "660f6ec00f5bc0f30f5e054e9a9b03f30f58c60f28f0",
    interpretation: "singing converts signed attribute 11730 i32 to float32, divides by float32 10000, then adds float32 1",
  },
  {
    id: "singing-no-match-temporary-float32-operation-order",
    rva: 0x5295410,
    bytes: "0f57c0f30f5e05f1999b03f30f58f00f28c6",
    interpretation: "a missing singing temporary lane contributes float32 zero divided by float32 10000 and adds that result to the prior float32 speed",
  },
  {
    id: "singing-matching-temporary-float32-operation-order",
    rva: 0x5295424,
    bytes: "660f6e4424700f5bc0f30f5e05d7999b03f30f58f00f28c6",
    interpretation: "a matching singing temporary signed i32 is converted to float32, divided by float32 10000, then added to the prior float32 speed",
  },
];

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

function argumentsFrom(argv) {
  const values = [...argv];
  const binary = path.resolve(take(values, "--binary"));
  const identity = path.resolve(take(values, "--identity"));
  const dump = path.resolve(take(values, "--dump"));
  const output = path.resolve(take(values, "--output"));
  const build = take(values, "--build");
  if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
  return { binary, identity, dump, output, build };
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function artifact(file, bytes) {
  return { file, bytes: bytes.length, sha256: sha256(bytes) };
}

function parsePe(bytes) {
  if (bytes.toString("ascii", 0, 2) !== "MZ") fail("GameAssembly is not a PE image");
  const pe = bytes.readUInt32LE(0x3c);
  if (bytes.toString("ascii", pe, pe + 4) !== "PE\0\0") fail("GameAssembly has no PE signature");
  const sectionCount = bytes.readUInt16LE(pe + 6);
  const optionalHeaderBytes = bytes.readUInt16LE(pe + 20);
  const sectionTable = pe + 24 + optionalHeaderBytes;
  const sections = [];
  for (let index = 0; index < sectionCount; index += 1) {
    const offset = sectionTable + index * 40;
    sections.push({
      name: bytes.toString("ascii", offset, offset + 8).replaceAll("\0", ""),
      virtual_size: bytes.readUInt32LE(offset + 8),
      virtual_address: bytes.readUInt32LE(offset + 12),
      raw_size: bytes.readUInt32LE(offset + 16),
      raw_pointer: bytes.readUInt32LE(offset + 20),
    });
  }
  return sections;
}

function offsetForRva(sections, rva) {
  const section = sections.find(
    (candidate) =>
      rva >= candidate.virtual_address &&
      rva < candidate.virtual_address + Math.max(candidate.virtual_size, candidate.raw_size),
  );
  if (!section) fail(`RVA 0x${rva.toString(16)} is outside the PE sections`);
  return section.raw_pointer + rva - section.virtual_address;
}

function bytesAt(binary, sections, rva, length) {
  const offset = offsetForRva(sections, rva);
  if (offset + length > binary.length) fail(`RVA 0x${rva.toString(16)} exceeds the binary`);
  return binary.subarray(offset, offset + length);
}

function verifyDumpMethod(dump, [name, rva, signature]) {
  const rvaText = `RVA: 0x${rva.toString(16).toUpperCase()}`;
  const rvaIndex = dump.indexOf(rvaText);
  if (rvaIndex < 0) fail(`${name} is missing exact ${rvaText} from the IL2CPP dump`);
  const signatureIndex = dump.indexOf(signature, rvaIndex);
  if (signatureIndex < 0 || signatureIndex - rvaIndex > 300) {
    fail(`${name} signature is not adjacent to exact ${rvaText}`);
  }
  return { name, rva, signature };
}

function main() {
  const args = argumentsFrom(process.argv.slice(2));
  if (args.build !== EXPECTED_BUILD) fail(`this reviewed proof only supports build ${EXPECTED_BUILD}`);
  if (existsSync(args.output)) fail(`refusing to overwrite ${args.output}`);
  const binary = readFileSync(args.binary);
  const identityBytes = readFileSync(args.identity);
  const identity = JSON.parse(identityBytes);
  const dumpBytes = readFileSync(args.dump);
  const dump = dumpBytes.toString("utf8");

  if (
    identity.deployment !== EXPECTED_DEPLOYMENT ||
    String(identity.game_build) !== args.build ||
    Number(identity.game_assembly?.byte_length) !== EXPECTED_BINARY_BYTES ||
    String(identity.game_assembly?.sha256) !== EXPECTED_BINARY_SHA256
  ) {
    fail("client identity is not the reviewed exact current build");
  }
  if (binary.length !== EXPECTED_BINARY_BYTES || sha256(binary) !== EXPECTED_BINARY_SHA256) {
    fail("GameAssembly bytes do not match the reviewed exact current build");
  }

  const sections = parsePe(binary);
  const methods = METHODS.map((method) => {
    const bytes = bytesAt(binary, sections, method.rva, method.end_rva - method.rva);
    const observed = sha256(bytes);
    if (observed !== method.sha256) fail(`${method.name} method-region digest changed`);
    return {
      ...method,
      bytes: bytes.length,
      runtime_formula_authority: false,
    };
  });
  const anchors = REVIEWED_ANCHORS.map((anchor) => {
    const expected = Buffer.from(anchor.bytes, "hex");
    const observed = bytesAt(binary, sections, anchor.rva, expected.length);
    if (!observed.equals(expected)) fail(`${anchor.id} reviewed instruction bytes changed`);
    if (anchor.call_target_rva !== undefined) {
      const callOffset = anchor.call_offset ?? 0;
      const displacement = observed.readInt32LE(callOffset + 1);
      const target = anchor.rva + callOffset + 5 + displacement;
      if (target !== anchor.call_target_rva) fail(`${anchor.id} call target changed`);
    }
    if (anchor.relative_target_rva !== undefined) {
      const relativeOffset = anchor.relative_offset ?? 0;
      const displacement = observed.readInt32LE(relativeOffset + 1);
      const target = anchor.rva + relativeOffset + 5 + displacement;
      if (target !== anchor.relative_target_rva) fail(`${anchor.id} relative target changed`);
    }
    return { ...anchor, instruction_bytes_sha256: sha256(observed) };
  });
  const fixedPointScale = bytesAt(binary, sections, FIXED_POINT_SCALE_RVA, 4).readFloatLE(0);
  const one = bytesAt(binary, sections, ONE_FLOAT_RVA, 4).readFloatLE(0);
  if (fixedPointScale !== 10_000 || one !== 1) fail("reviewed native float constants changed");
  const dumpMethods = DUMP_METHODS.map((method) => verifyDumpMethod(dump, method));
  const conflictingDumpLabels = CONFLICTING_DUMP_LABELS.map((method) =>
    verifyDumpMethod(dump, method),
  );

  const result = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-action-speed-current-build-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: EXPECTED_DEPLOYMENT,
    game_build: args.build,
    proof_state: "exact-current-build-native-action-speed-float32-operation-order-proven-runtime-join-open",
    inputs: {
      client_binary_identity: artifact(args.identity, identityBytes),
      game_assembly: artifact(args.binary, binary),
      il2cpp_dump: artifact(args.dump, dumpBytes),
    },
    policy: {
      executes_game_code: false,
      exact_numeric_attribute_and_effect_ids_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      historical_formula_proofs_are_current_build_identity: false,
      current_build_method_regions_are_digest_locked: true,
      current_build_reviewed_instruction_anchors_are_required: true,
      missing_or_unobserved_values_are_zero: false,
      remote_player_cast_packets_required: false,
      inferred_actions_from_damage_hits_allowed: false,
      current_character_snapshot_backfill_allowed: false,
      ordinary_damage_is_retained: true,
      provider_rdps_credit_allowed: false,
    },
    fixed_point_constants: {
      scale: { rva: FIXED_POINT_SCALE_RVA, value: fixedPointScale },
      one: { rva: ONE_FLOAT_RVA, value: one },
    },
    dump_methods: dumpMethods,
    conflicting_dump_labels: {
      labels: conflictingDumpLabels,
      labels_are_native_semantic_authority: false,
      exact_call_abi_and_float_consumption_override_labels: true,
    },
    temporary_attribute_lookup: {
      semantic_operation: "TryGetTempAttrByType",
      exact_native_call_target_rva: 0x4140810,
      receiver_argument: "temporary_attribute_component",
      effect_type_argument: {
        register: "RDX",
        normal: 700,
        guide: 710,
      },
      logic_type_argument: {
        register: "R8D",
        numeric_id: 1,
        exact_enum_member: "ETempAttrType.TempAttrSkill",
      },
      parameter_argument: {
        register: "R9D",
        value: "skill_id",
      },
      out_value_argument: 5,
      match_operation: "signed_i32_sum_of_every_entry_matching_effect_type_logic_type_and_skill_id",
      output_initialized_to_zero_before_lookup: true,
      no_match_returns_false_with_zero_output: true,
      dump_label_at_call_target_is_semantically_compatible: false,
      exact_native_abi_is_authoritative: true,
    },
    reviewed_method_regions: methods,
    reviewed_instruction_anchors: anchors,
    stage_families: {
      normal: {
        stage_type: 0,
        enabled_by: "SkillTable.AtkSpeedSwitch",
        attribute_id: 11720,
        temporary_effect_type: 700,
        exact_algebraic_speed: "(10000 + attribute_11720 + temporary_effect_700) / 10000",
      },
      singing: {
        stage_type: 1,
        attribute_id: 11730,
        temporary_effect_type: 710,
        exact_native_branch_proven: true,
        exact_native_float32_operation_order:
          "add_f32(1.0f, div_f32(i32_to_f32(attribute_11730), 10000.0f)); then add_f32(previous, div_f32(i32_to_f32(matching_temporary_effect_710_or_zero), 10000.0f))",
        float_boundary_equivalence_for_offline_rational_replay_proven: false,
      },
      charge: {
        stage_type: 2,
        attribute_id: 11740,
        exact_algebraic_speed: "(10000 + attribute_11740) / 10000",
      },
      guide: {
        stage_types: [8, 9],
        attribute_id: 11730,
        temporary_effect_type: 710,
        exact_algebraic_speed: "(10000 + attribute_11730 + temporary_effect_710) / 10000",
      },
      pet: {
        entity_type_id: 8,
        selected_before_stage_type: true,
        attribute_id: 11990,
        exact_algebraic_speed: "(10000 + attribute_11990) / 10000",
      },
      unaffected: {
        exact_speed: "1 / 1",
      },
    },
    summary: {
      exact_current_build_binary_identity: true,
      exact_current_build_method_identity: true,
      exact_current_build_stage_selection_proven: true,
      exact_skill_scoped_temporary_attribute_lookup_abi_proven: true,
      exact_temporary_attribute_match_operation_and_no_match_zero_proven: true,
      packet_owner_stage_to_stage_type_mapping_proven: false,
      exact_non_singing_algebraic_speed_formulas_proven: true,
      exact_native_float32_operation_order_proven: true,
      singing_native_float32_operation_order_proven: true,
      singing_offline_numeric_equivalence_proven: false,
      exact_action_time_attribute_snapshot_route_proven: false,
      exact_action_to_damage_ancestry_proven: false,
      exact_provider_removed_speed_replay_proven: false,
      exact_integer_damage_rounding_proven: false,
      packet_conservation_proven: false,
      runtime_promotion_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    blockers: [
      "action-time speed attributes and temporary effect lanes are not yet joined to each exact damage action",
      "damage actions are not yet joined to exact SkillTable stage rows and AtkSpeedSwitch for the recording build",
      "the IL2CPP dump labels at RVAs 0x52951b0 and 0x5295240 conflict with the exact native call ABI and cannot prove owner_stage to EStageType mapping",
      "the exact native float32 operation order is proven, but offline bit-equivalent replay remains closed until the runtime floating-point environment and every input snapshot are proven",
      "provider-removed action speed and action-linked conserved damage replay remain unproven",
      "current-build protocol pack identity and required replay gates remain missing",
    ],
  };
  writeFileSync(args.output, `${JSON.stringify(result, null, 2)}\n`);
  process.stdout.write(`wrote ${args.output}\n`);
}

main();
