#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import readline from "node:readline";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-autoattack-wire-roll-frontier.mjs";
const SCHEMA_VERSION = 2;
const GAME_BUILD = "24687926";
const ABILITY_ID = 2_900_840;
const SUPPORT_EFFECT_ID = 2_110_140;
const EXACT_DAMAGE_ATTR_IDS = [
  "129008400103",
  "129008400105",
  "129008400107",
  "129008400108",
  "129008400109",
];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") await build(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function build(values) {
  const cohortPath = path.resolve(required(values, "cohort"));
  const coefficientProofPath = path.resolve(required(values, "coefficient-proof"));
  const nativeGetterCallsitesPath = path.resolve(
    required(values, "native-getter-callsites"),
  );
  const damageAttrPath = path.resolve(required(values, "damage-attr"));
  const il2cppDumpPath = path.resolve(required(values, "il2cpp-dump"));
  const journalPath = path.resolve(required(values, "journal"));
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const cohort = readJson(cohortPath);
  const coefficientProof = readJson(coefficientProofPath);
  const nativeGetterCallsites = readJson(nativeGetterCallsitesPath);
  const damageAttrs = readJson(damageAttrPath);
  validateInputs(cohort, coefficientProof, nativeGetterCallsites, damageAttrs);

  const responseSurface = summarizeResponseSurface(cohort.samples);
  const sameWire = summarizeSameWireProof(coefficientProof);
  const exactRows = summarizeDamageAttrRows(damageAttrs);
  const nativeSurface = await scanNativeSurface(il2cppDumpPath);
  const journal = await scanJournal(journalPath);

  const output = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    identity: {
      ability_id: ABILITY_ID,
      support_effect_id: SUPPORT_EFFECT_ID,
      damage_script: "AutoAttack",
      exact_damage_attr_ids: EXACT_DAMAGE_ATTR_IDS,
    },
    sources: {
      formula_cohort: await fileReceipt(cohortPath),
      same_wire_coefficient_proof: await fileReceipt(coefficientProofPath),
      native_getter_callsites: await fileReceipt(nativeGetterCallsitesPath),
      damage_attr_table: await fileReceipt(damageAttrPath),
      il2cpp_dump: await fileReceipt(il2cppDumpPath),
      raw_protocol_journal: await fileReceipt(journalPath),
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      remote_player_cast_packets_required: false,
      unresolved_formula_inputs_preserved: true,
      packet_field_names_are_formula_authority: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
    exact_damage_attr_rows: exactRows,
    sync_damage_response_surface: responseSurface,
    exact_build_native_surface: nativeSurface,
    same_wire_component_discriminant: sameWire,
    selected_capture_client_direction: journal,
    conclusion: {
      sync_damage_weight_is_usable_ability_2900840_roll_input: false,
      reason:
        "All retained ability-2900840 SyncDamageInfo DamageWeight components are absent; exact-build native metadata places the same-named vector beside hit/stiffness presentation fields and randDamageWeight/prePlayStiff.",
      same_wire_components_prove_one_shared_hidden_roll: false,
      same_wire_reason:
        "Exact hit IDs 7 and 8 use identical current-build coefficient/fixed rows, yet three of ten otherwise retained same-wire pairs have different normal damage values.",
      client_hit_request_has_direct_roll_field: false,
      client_hit_request_part_damage_value_disposition:
        "ClientHitPartInfo.DamageVal exists, but this selected journal contains no payload occurrence of ability 2900840 in the retained client-to-server packets. Its relationship to authoritative server damage is unresolved.",
      direct_client_table_getter_consumer_found: false,
      exact_server_integer_rounding_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      smallest_next_proof:
        "Acquire or instrument one local client-originated SyncHitInfo/SyncBulletHitInfo lifecycle and correlate ClientHitPartInfo.DamageVal with its exact SyncDamageInfo response, or recover the authoritative server AutoAttack operator. Do not require unavailable remote-player cast packets.",
    },
    peak_working_set_bytes: process.memoryUsage().rss,
  };
  output.content_sha256 = contentSha256(output);
  verifyReport(output);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote ${outputPath}`);
}

function validateInputs(cohort, proof, callsites, damageAttrs) {
  assert.equal(String(cohort.game_build), GAME_BUILD);
  assert.equal(cohort.schema_version, 46);
  assert.deepEqual(cohort.selection.ability_ids, [ABILITY_ID]);
  assert.ok(Array.isArray(cohort.samples) && cohort.samples.length > 0);
  assert.equal(String(proof.game_build), GAME_BUILD);
  assert.equal(String(proof.packet_build), GAME_BUILD);
  assert.equal(proof.schema_version, 11);
  assert.ok(proof.policy.coefficient_families.includes("AutoAttack"));
  assert.equal(String(callsites.game_build), GAME_BUILD);
  assert.equal(callsites.summary.direct_callsites, 0);
  for (const id of EXACT_DAMAGE_ATTR_IDS) assert.ok(damageAttrs[id], `missing ${id}`);
}

function summarizeDamageAttrRows(rows) {
  return EXACT_DAMAGE_ATTR_IDS.map((id) => {
    const row = rows[id];
    assert.equal(row.DamageScript, "AutoAttack");
    assert.deepEqual(row.DamageWeight, []);
    return {
      damage_attr_id: id,
      damage_type: row.DamageType,
      pve_damage_ratio: row.PVEDamageRadio,
      pve_fixed_parameter: row.PVEFixedParameter,
      damage_weight: row.DamageWeight,
    };
  });
}

function summarizeResponseSurface(samples) {
  const present = (value) => value !== null && value !== undefined;
  const count = (selector) => samples.reduce((sum, sample) => sum + Number(present(selector(sample))), 0);
  return {
    samples: samples.length,
    normal_value_present: count((sample) => sample.packet?.normal_value),
    actual_amount_present: count((sample) => sample.actual_amount),
    lucky_value_present: count((sample) => sample.packet?.lucky_value),
    damage_weight_x_present: count((sample) => sample.packet?.damage_weight?.x),
    damage_weight_y_present: count((sample) => sample.packet?.damage_weight?.y),
    retained_hit_part_rows: samples.reduce(
      (sum, sample) => sum + (sample.packet?.hit_parts?.length ?? 0),
      0,
    ),
    packet_damage_mode_values: [...new Set(samples.map((sample) => sample.packet?.damage_mode))],
    formula_authority: false,
  };
}

function summarizeSameWireProof(proof) {
  const pair = proof.sibling_pairs.find(
    (candidate) =>
      candidate.ability_id === ABILITY_ID &&
      candidate.first_hit_event_id === 7 &&
      candidate.second_hit_event_id === 8,
  );
  assert.ok(pair, "missing exact ability-2900840 hit-7/hit-8 pair");
  assert.equal(pair.first_damage_id, "129008400107");
  assert.equal(pair.second_damage_id, "129008400108");
  const candidateCounts = pair.candidates.map((candidate) => ({
    shared_array_index: candidate.shared_array_index,
    coefficient_pairs: candidate.coefficient_pairs,
    equal_normal_pairs: candidate.exact_normal_proportions,
    divergent_normal_pairs:
      pair.comparable_normal_values - candidate.exact_normal_proportions,
    integer_floor_compatible_pairs: candidate.integer_floor_compatible_normal_pairs,
    integer_floor_compatible_for_every_pair:
      candidate.integer_floor_compatible_for_every_comparable_normal_pair,
  }));
  assert.ok(candidateCounts.every((candidate) => candidate.divergent_normal_pairs > 0));
  return {
    ability_id: pair.ability_id,
    hit_event_ids: [pair.first_hit_event_id, pair.second_hit_event_id],
    damage_attr_ids: [pair.first_damage_id, pair.second_damage_id],
    paired_events: pair.paired_events,
    comparable_normal_values: pair.comparable_normal_values,
    candidates: candidateCounts,
    shared_hidden_roll_proven: false,
    formula_authority: false,
  };
}

async function scanNativeSurface(inputPath) {
  const fields = {
    client_hit_info: new Set(),
    client_hit_part_info: new Set(),
    sync_damage_info: new Set(),
  };
  let active = null;
  let randDamageWeight = false;
  let prePlayStiff = false;
  const stream = fs.createReadStream(inputPath, { encoding: "utf8" });
  const lines = readline.createInterface({ input: stream, crlfDelay: Infinity });
  for await (const line of lines) {
    if (line.startsWith("public sealed class ClientHitInfo :")) active = "client_hit_info";
    else if (line.startsWith("public sealed class ClientHitPartInfo :")) active = "client_hit_part_info";
    else if (line.startsWith("public sealed class SyncDamageInfo :")) active = "sync_damage_info";
    else if (active && line.trim() === "// Properties") active = null;
    else if (active) {
      const match = line.match(/^\s*public\s+[^;]+\s+([A-Za-z0-9_]+);/);
      if (match) fields[active].add(match[1]);
    }
    if (line.includes("randDamageWeight(")) {
      randDamageWeight = true;
    }
    if (line.includes("prePlayStiff(")) prePlayStiff = true;
  }
  assert.ok(fields.client_hit_info.has("PartInfos"));
  assert.ok(!fields.client_hit_info.has("DamageWeight"));
  assert.ok(fields.client_hit_part_info.has("DamageVal"));
  assert.ok(fields.sync_damage_info.has("DamageWeight"));
  assert.ok(randDamageWeight && prePlayStiff);
  return {
    client_hit_info_fields: [...fields.client_hit_info],
    client_hit_info_has_damage_weight: fields.client_hit_info.has("DamageWeight"),
    client_hit_part_info_fields: [...fields.client_hit_part_info],
    client_hit_part_info_has_damage_value: fields.client_hit_part_info.has("DamageVal"),
    sync_damage_info_fields: [...fields.sync_damage_info],
    sync_damage_info_has_damage_weight: fields.sync_damage_info.has("DamageWeight"),
    native_hit_presentation_methods: {
      rand_damage_weight: randDamageWeight,
      pre_play_stiff: prePlayStiff,
    },
    semantic_authority: "field-and-method identity only; no server arithmetic is inferred",
  };
}

async function scanJournal(inputPath) {
  const directionCounts = new Map();
  const clientRoutes = new Map();
  const abilityVarint = encodeVarint(ABILITY_ID);
  let clientPayloadsContainingAbilityVarint = 0;
  const stream = fs.createReadStream(inputPath, { encoding: "utf8" });
  const lines = readline.createInterface({ input: stream, crlfDelay: Infinity });
  for await (const line of lines) {
    const record = JSON.parse(line);
    if (record?.line !== "record" || record?.data?.kind?.record !== "packet") continue;
    const packet = record.data.kind.data;
    directionCounts.set(packet.direction, (directionCounts.get(packet.direction) ?? 0) + 1);
    if (packet.direction !== "client_to_server") continue;
    const key = [
      packet.fragment?.kind,
      packet.route?.key?.service_id,
      packet.route?.key?.method_id,
    ].join(":");
    clientRoutes.set(key, (clientRoutes.get(key) ?? 0) + 1);
    const payload = packet.payload?.application_bytes ?? [];
    if (containsSubsequence(payload, abilityVarint)) clientPayloadsContainingAbilityVarint += 1;
  }
  return {
    direction_packet_counts: Object.fromEntries([...directionCounts].sort()),
    client_to_server_route_counts: Object.fromEntries([...clientRoutes].sort()),
    client_payloads_containing_ability_2900840_varint: clientPayloadsContainingAbilityVarint,
    client_hit_request_for_exact_ability_observed: false,
    authority_boundary:
      "A zero varint occurrence does not prove every client RPC type absent; it proves only that the exact ability integer was not retained in this journal's client payloads.",
  };
}

function encodeVarint(value) {
  const bytes = [];
  let remaining = BigInt(value);
  while (remaining >= 0x80n) {
    bytes.push(Number((remaining & 0x7fn) | 0x80n));
    remaining >>= 7n;
  }
  bytes.push(Number(remaining));
  return bytes;
}

function containsSubsequence(values, needle) {
  outer: for (let start = 0; start <= values.length - needle.length; start += 1) {
    for (let offset = 0; offset < needle.length; offset += 1) {
      if (values[start + offset] !== needle[offset]) continue outer;
    }
    return true;
  }
  return false;
}

async function fileReceipt(inputPath) {
  const stat = fs.statSync(inputPath);
  const hash = crypto.createHash("sha256");
  for await (const chunk of fs.createReadStream(inputPath)) hash.update(chunk);
  return {
    path: inputPath.replaceAll("\\", "/"),
    bytes: stat.size,
    sha256: hash.digest("hex"),
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
  assert.equal(String(report.game_build), GAME_BUILD);
  assert.equal(report.identity.ability_id, ABILITY_ID);
  assert.equal(report.sync_damage_response_surface.damage_weight_x_present, 0);
  assert.equal(report.sync_damage_response_surface.damage_weight_y_present, 0);
  assert.equal(report.sync_damage_response_surface.actual_amount_present, 0);
  assert.equal(report.sync_damage_response_surface.retained_hit_part_rows, 0);
  assert.ok(report.same_wire_component_discriminant.paired_events >= 2);
  assert.ok(
    report.same_wire_component_discriminant.candidates.every(
      (candidate) => candidate.divergent_normal_pairs > 0,
    ),
  );
  assert.equal(report.exact_build_native_surface.client_hit_info_has_damage_weight, false);
  assert.equal(report.exact_build_native_surface.client_hit_part_info_has_damage_value, true);
  assert.equal(report.selected_capture_client_direction.client_payloads_containing_ability_2900840_varint, 0);
  assert.equal(report.conclusion.exact_server_integer_rounding_proven, false);
  assert.equal(report.conclusion.provider_rdps_credit_allowed, false);
  assert.equal(report.conclusion.runtime_promotion_allowed, false);
  assert.equal(report.content_sha256, contentSha256(report));
}

function selfTest() {
  assert.deepEqual(encodeVarint(ABILITY_ID), [232, 134, 177, 1]);
  assert.equal(containsSubsequence([1, 232, 134, 177, 1, 2], encodeVarint(ABILITY_ID)), true);
  assert.equal(containsSubsequence([232, 134, 1], encodeVarint(ABILITY_ID)), false);
  console.log("self-test passed");
}

function contentSha256(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function readJson(inputPath) {
  return JSON.parse(fs.readFileSync(inputPath, "utf8"));
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
      "  node tools/bpsr-autoattack-wire-roll-frontier.mjs build --cohort FILE --coefficient-proof FILE --native-getter-callsites FILE --damage-attr FILE --il2cpp-dump FILE --journal FILE --output FILE\n" +
      "  node tools/bpsr-autoattack-wire-roll-frontier.mjs verify --input FILE\n" +
      "  node tools/bpsr-autoattack-wire-roll-frontier.mjs self-test",
  );
  process.exit(exitCode);
}
