#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-client-hit-damageval-native-frontier.mjs";
const SCHEMA_VERSION = 1;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(values) {
  const identityPath = path.resolve(required(values, "identity"));
  const wireProofPath = path.resolve(required(values, "native-wire-proof"));
  const callsitesPath = path.resolve(required(values, "direct-callsites"));
  const historicalPath = path.resolve(required(values, "historical-wire-candidate"));
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const identity = readJson(identityPath);
  const wireProof = readJson(wireProofPath);
  const callsites = readJson(callsitesPath);
  const historical = readJson(historicalPath);
  const gameBuild = String(identity.game_build);

  assert.equal(wireProof.game_build, gameBuild);
  assert.equal(String(callsites.game_build), gameBuild);
  assert.equal(callsites.binary.sha256, identity.game_assembly.sha256);
  assert.equal(callsites.binary.byte_length, identity.game_assembly.byte_length);
  assert.equal(historical.game_build, "24252055");
  assert.notEqual(historical.game_build, gameBuild);

  const clientHit = exactMessage(wireProof, "Zproto.ClientHitInfo");
  const clientHitPart = exactMessage(wireProof, "Zproto.ClientHitPartInfo");
  const partInfos = exactField(clientHit, "PartInfos", 13, [106], [2]);
  const damageVal = exactField(clientHitPart, "DamageVal", 3, [24], [0]);

  assert.equal(callsites.summary.direct_callsites, 2);
  const gameplay = exactCallsite(callsites, "Panda.ZGame.ZHitMgr$$buildDamageInfo");
  const clone = exactCallsite(callsites, "Zproto.ClientHitPartInfo$$Clone");
  assert.equal(gameplay.target_names.includes("Zproto.ClientHitPartInfo$$Rent"), true);
  assert.equal(clone.target_names.includes("Zproto.ClientHitPartInfo$$Rent"), true);

  const gameplayPrefix = constructionPrefix(gameplay);
  const clonePrefix = constructionPrefix(clone);
  assert.deepEqual(gameplayPrefix.written_offsets_hex, ["0x10", "0x14", "0x18", "0x1C"]);
  assert.equal(gameplayPrefix.damage_val_offset_written, false);
  assert.equal(clonePrefix.written_offsets_hex.includes("0x20"), true);
  assert.equal(historical.summary.client_hit_family_candidate_packets, 1564);
  assert.equal(historical.summary.client_hit_family_candidate_rows, 3249);
  assert.equal(historical.summary.client_hit_part_candidate_rows, 0);
  assert.equal(historical.summary.part_field_3_candidate_values, 0);

  const output = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    sources: {
      client_binary_identity: fileReceipt(identityPath),
      native_wire_proof: fileReceipt(wireProofPath),
      direct_callsite_audit: fileReceipt(callsitesPath),
      historical_wire_candidate: fileReceipt(historicalPath),
    },
    policy: {
      exact_build_identity_required_for_native_claims: true,
      historical_build_used_as_current_formula_authority: false,
      direct_calls_claimed_complete_for_selected_exact_rva: true,
      indirect_calls_claimed_absent: false,
      field_default_interpreted_as_formula_value: false,
      runtime_promotion_allowed: false,
      provider_rdps_credit_allowed: false,
    },
    exact_current_build_wire_identity: {
      client_hit_part_infos: fieldSummary(partInfos),
      client_hit_part_damage_val: fieldSummary(damageVal),
    },
    exact_current_build_direct_rent_callsites: {
      target_rva: gameplay.target_rva,
      target_names: gameplay.target_names,
      count: callsites.summary.direct_callsites,
      gameplay_constructor: {
        caller_names: gameplay.caller.names,
        call_rva: gameplay.call_rva,
        construction_prefix: gameplayPrefix,
      },
      protobuf_clone: {
        caller_names: clone.caller.names,
        call_rva: clone.call_rva,
        construction_prefix: clonePrefix,
      },
    },
    historical_build_observation: {
      game_build: historical.game_build,
      current_build_authority: false,
      client_hit_family_candidate_packets: historical.summary.client_hit_family_candidate_packets,
      client_hit_family_candidate_rows: historical.summary.client_hit_family_candidate_rows,
      client_hit_part_candidate_rows: historical.summary.client_hit_part_candidate_rows,
      part_field_3_candidate_values: historical.summary.part_field_3_candidate_values,
    },
    conclusion: {
      current_build_part_infos_tag_proven: true,
      current_build_damage_val_tag_proven: true,
      gameplay_constructor_writes_part_id_and_damage_position: true,
      gameplay_constructor_direct_damage_val_write_observed: false,
      protobuf_clone_preserves_existing_damage_val: true,
      damage_val_is_authoritative_server_formula_input_proven: false,
      damage_val_is_viable_immediate_operator_frontier: false,
      exact_server_operator_or_rounding_proven: false,
      smallest_next_proof:
        "Use the forward-retained current-build ClientHitInfo request for actor/target/action/time correlation only. Align it to the exact SyncDamageInfo response and the already proven event-time Attack transition; do not require ClientHitPartInfo.DamageVal or unavailable remote-player cast packets. An authoritative server operator or a controlled same-context Attack transition is still required for arithmetic promotion.",
      runtime_promotion_allowed: false,
      provider_rdps_credit_allowed: false,
    },
    peak_working_set_bytes: process.memoryUsage().rss,
  };
  output.content_sha256 = contentSha256(output);
  verifyReport(output);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote ${outputPath}`);
}

function exactMessage(proof, fullName) {
  assert.equal(proof.generated_by, "rlogs-bpsr-protobuf-native-wire-proof");
  assert.equal(proof.policy.generated_field_order_used_as_tag, false);
  const message = proof.messages.find((candidate) => candidate.full_name === fullName);
  assert.ok(message, `missing ${fullName}`);
  assert.equal(message.state, "exact");
  assert.equal(message.field_count, message.exact_field_tags);
  return message;
}

function exactField(message, name, tag, wireKeys, wireTypes) {
  const field = message.fields.find((candidate) => candidate.name === name);
  assert.ok(field, `missing ${message.full_name}.${name}`);
  assert.equal(field.protobuf_tag, tag);
  assert.deepEqual(field.accepted_wire_keys_decimal, wireKeys);
  assert.deepEqual(field.accepted_wire_types, wireTypes);
  assert.equal(field.proof_state, "exact_native_merge_branch");
  return field;
}

function exactCallsite(audit, callerName) {
  const matches = audit.callsites.filter((candidate) => candidate.caller.names.includes(callerName));
  assert.equal(matches.length, 1, `expected one direct callsite in ${callerName}`);
  return matches[0];
}

function constructionPrefix(callsite) {
  const instructions = callsite.disassembly;
  const callIndex = instructions.findIndex((row) => row.rva === callsite.call_rva && row.is_target_call === true);
  assert.ok(callIndex >= 0, "target call absent from disassembly");
  const prefix = [];
  for (let index = callIndex + 1; index < instructions.length && prefix.length < 16; index += 1) {
    prefix.push(instructions[index]);
    if (instructions[index].mnemonic === "call") break;
  }
  const writes = [];
  for (const row of prefix) {
    if (!row.mnemonic.startsWith("mov")) continue;
    const match = row.operands.match(/^.+ ptr \[(?:rax|rdi|rdx) \+ 0x([0-9a-f]+)\],/i);
    if (match) writes.push(Number.parseInt(match[1], 16));
  }
  return {
    bounded_instruction_count: prefix.length,
    written_offsets_hex: [...new Set(writes)].sort((a, b) => a - b).map(hex),
    damage_val_offset_written: writes.includes(0x20),
    instructions: prefix,
  };
}

function fieldSummary(field) {
  return {
    name: field.name,
    field_type: field.field_type,
    protobuf_tag: field.protobuf_tag,
    accepted_wire_keys_decimal: field.accepted_wire_keys_decimal,
    accepted_wire_types: field.accepted_wire_types,
    proof_state: field.proof_state,
  };
}

function verify(values) {
  const inputPath = path.resolve(required(values, "input"));
  const report = readJson(inputPath);
  verifyReport(report);
  console.log(`verified ${inputPath}`);
}

function verifyReport(report) {
  assert.equal(report.schema_version, SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.exact_current_build_wire_identity.client_hit_part_infos.protobuf_tag, 13);
  assert.equal(report.exact_current_build_wire_identity.client_hit_part_damage_val.protobuf_tag, 3);
  assert.equal(
    report.exact_current_build_direct_rent_callsites.gameplay_constructor.construction_prefix.damage_val_offset_written,
    false,
  );
  assert.equal(
    report.exact_current_build_direct_rent_callsites.protobuf_clone.construction_prefix.damage_val_offset_written,
    true,
  );
  assert.equal(report.historical_build_observation.current_build_authority, false);
  assert.equal(report.conclusion.damage_val_is_viable_immediate_operator_frontier, false);
  assert.equal(report.conclusion.runtime_promotion_allowed, false);
  assert.equal(report.conclusion.provider_rdps_credit_allowed, false);
  assert.equal(report.content_sha256, contentSha256(report));
}

function selfTest() {
  const sample = {
    call_rva: 10,
    disassembly: [
      { rva: 10, mnemonic: "call", operands: "0x20", is_target_call: true },
      { rva: 15, mnemonic: "mov", operands: "rdi, rax", is_target_call: false },
      { rva: 18, mnemonic: "mov", operands: "dword ptr [rax + 0x10], ebx", is_target_call: false },
      { rva: 21, mnemonic: "mov", operands: "qword ptr [rdi + 0x20], rcx", is_target_call: false },
      { rva: 25, mnemonic: "call", operands: "0x30", is_target_call: false },
    ],
  };
  const prefix = constructionPrefix(sample);
  assert.deepEqual(prefix.written_offsets_hex, ["0x10", "0x20"]);
  assert.equal(prefix.damage_val_offset_written, true);
  console.log("self-test passed");
}

function readJson(inputPath) {
  return JSON.parse(fs.readFileSync(inputPath, "utf8"));
}

function fileReceipt(inputPath) {
  const bytes = fs.readFileSync(inputPath);
  return {
    path: inputPath.replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function contentSha256(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function hex(value) {
  return `0x${value.toString(16).toUpperCase()}`;
}

function refuseExisting(outputPath) {
  if (fs.existsSync(outputPath)) throw new Error(`refusing to overwrite ${outputPath}`);
}

function parseArgs(args) {
  const values = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined) usage(1);
    values.set(flag.slice(2), value);
  }
  return values;
}

function required(values, key) {
  const value = values.get(key);
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function usage(exitCode) {
  console.error(
    "Usage:\n" +
      "  node tools/bpsr-client-hit-damageval-native-frontier.mjs build --identity FILE --native-wire-proof FILE --direct-callsites FILE --historical-wire-candidate FILE --output FILE\n" +
      "  node tools/bpsr-client-hit-damageval-native-frontier.mjs verify --input FILE\n" +
      "  node tools/bpsr-client-hit-damageval-native-frontier.mjs self-test",
  );
  process.exit(exitCode);
}
