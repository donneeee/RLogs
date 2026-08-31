#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const GENERATED_BY = "tools/bpsr-client-hit-wire-candidate.mjs";
const SCHEMA_VERSION = 3;
const SUPPORTED_REPORT_SCHEMA_VERSIONS = new Set([2, SCHEMA_VERSION]);
const ACTION_TIMELINE_SCHEMA_VERSION = 1;
const WORLD_SERVICE_ID = 103_198_054;
const CLIENT_HIT_WIRE_TYPES = new Map([
  [1, 0],
  [2, 0],
  [3, 0],
  [4, 0],
  [5, 0],
  [6, 0],
  [7, 0],
  [8, 0],
  [9, 0],
  [10, 2],
  [11, 2],
  [12, 2],
  [13, 2],
  [14, 0],
  [15, 0],
]);
const REQUIRED_CLIENT_HIT_FIELDS = [8, 9, 10, 11, 12];
const MAX_EXAMPLES_PER_ROUTE = 5;
const MAX_DECODED_ACTION_EXAMPLES_PER_ROUTE = 20;
const MAX_VALUES_PER_FIELD = 64;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") await build(options);
else if (command === "verify") verify(options);
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function build(values) {
  const journalPath = path.resolve(required(values, "journal"));
  const dumpPath = path.resolve(required(values, "il2cpp-dump"));
  const gameBuild = required(values, "game-build");
  const nativeWireProofPath = values.get("native-wire-proof")
    ? path.resolve(values.get("native-wire-proof"))
    : null;
  const actionTimelinePath = values.get("actions-output")
    ? path.resolve(values.get("actions-output"))
    : null;
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);
  if (actionTimelinePath) {
    assert.ok(
      nativeWireProofPath,
      "--actions-output requires --native-wire-proof from the exact same build",
    );
    refuseExisting(actionTimelinePath);
    refuseExisting(`${actionTimelinePath}.partial`);
  }

  const native = await scanNativeSurface(dumpPath);
  const tagAuthority = nativeWireProofPath
    ? loadExactTagAuthority(nativeWireProofPath, gameBuild)
    : null;
  const wire = await scanJournal(journalPath, tagAuthority !== null, {
    actionTimelinePath,
    gameBuild,
  });
  const output = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    sources: {
      protocol_journal: wire.source,
      il2cpp_dump: await fileReceipt(dumpPath),
      native_wire_proof: nativeWireProofPath ? await fileReceipt(nativeWireProofPath) : null,
      local_action_timeline: wire.action_timeline_source,
    },
    policy: {
      exact_numeric_build_and_route_ids_authoritative: true,
      generated_field_order_is_protobuf_tag_authority: false,
      individual_proxy_method_names_assigned: false,
      raw_payloads_copied_to_report: false,
      complete_decoded_local_action_rows_streamed: actionTimelinePath !== null,
      local_action_timeline_infers_actor_allegiance: false,
      local_action_timeline_infers_provider_ownership: false,
      unresolved_routes_preserved: true,
      remote_player_cast_packets_required: false,
      formula_authority: false,
      runtime_promotion_allowed: false,
      provider_rdps_credit_allowed: false,
    },
    exact_build_native_surface: native,
    summary: wire.summary,
    client_hit_family_candidate_routes: wire.routes,
    conclusion: {
      client_hit_family_wire_shape_observed: wire.routes.length > 0,
      individual_sync_hit_method_mapping_proven: false,
      client_hit_part_damage_value_tag_proven: tagAuthority !== null,
      complete_local_action_timeline_emitted: wire.action_timeline_source !== null,
      client_hit_part_field_3_candidate_values_observed: wire.summary.part_field_3_candidate_values > 0,
      exact_server_relation_to_part_field_3_proven: false,
      smallest_next_proof: tagAuthority
        ? "Align one exact-current-build decoded local request row to its exact SyncDamageInfo response by actor, target, attack UUID, numeric action ID, hit event, and time. Treat DamageVal as optional evidence only; the ordinary current-build gameplay constructor does not directly populate it."
        : "Repeat this scan on an exact-current-build controlled local direct-skill capture with the matching native wire proof. Preserve the anonymous candidate rows, but do not assign field names across builds.",
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

async function scanNativeSurface(inputPath) {
  const classes = new Map([
    ["ClientHitInfo", []],
    ["ClientHitPartInfo", []],
  ]);
  let active = null;
  const input = fs.createReadStream(inputPath, { encoding: "utf8" });
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  for await (const line of lines) {
    const classMatch = line.match(/^public sealed class (ClientHitInfo|ClientHitPartInfo) :/);
    if (classMatch) active = classMatch[1];
    else if (active && line.trim() === "// Properties") active = null;
    else if (active) {
      const fieldMatch = line.match(/^\s*public\s+([^;]+)\s+([A-Za-z0-9_]+);/);
      if (fieldMatch) classes.get(active).push({ field_type: fieldMatch[1], name: fieldMatch[2] });
    }
  }
  const clientHitFields = classes.get("ClientHitInfo");
  const partFields = classes.get("ClientHitPartInfo");
  assert.deepEqual(clientHitFields.map((field) => field.name), [
    "SourceType",
    "Uuid",
    "Id",
    "Level",
    "Stage",
    "EventId",
    "AttackTime",
    "AttackUuid",
    "TargetUuid",
    "AttackPos",
    "TargetPos",
    "DamagePos",
    "PartInfos",
    "IsDodgeSuccess",
    "SkillUuid",
  ]);
  assert.deepEqual(partFields.map((field) => field.name), ["PartId", "DamagePos", "DamageVal"]);
  return {
    client_hit_info_fields_in_generated_instance_order: clientHitFields,
    client_hit_part_info_fields_in_generated_instance_order: partFields,
    protobuf_tags_available_in_native_metadata: false,
    semantic_authority: "exact class and field identity/order only; protobuf tag numbers and arithmetic remain unproven",
  };
}

function loadExactTagAuthority(inputPath, gameBuild) {
  const proof = JSON.parse(fs.readFileSync(inputPath, "utf8"));
  assert.equal(proof.generated_by, "rlogs-bpsr-protobuf-native-wire-proof");
  assert.equal(proof.game_build, gameBuild);
  assert.equal(proof.policy.generated_field_order_used_as_tag, false);
  const expected = new Map([
    ["Zproto.ClientHitInfo", CLIENT_HIT_WIRE_TYPES],
    ["Zproto.ClientHitPartInfo", new Map([[1, 0], [2, 2], [3, 0]])],
  ]);
  for (const [fullName, fields] of expected) {
    const message = proof.messages.find((candidate) => candidate.full_name === fullName);
    assert.ok(message, `native wire proof lacks ${fullName}`);
    assert.equal(message.state, "exact");
    assert.equal(message.exact_field_tags, message.field_count);
    for (const [tag, wireType] of fields) {
      const field = message.fields.find((candidate) => candidate.protobuf_tag === tag);
      assert.ok(field, `${fullName} lacks exact tag ${tag}`);
      assert.deepEqual(field.accepted_wire_types, [wireType]);
      assert.equal(field.proof_state, "exact_native_merge_branch");
    }
  }
  return proof;
}

async function scanJournal(
  inputPath,
  tagAuthority,
  { actionTimelinePath = null, gameBuild = null } = {},
) {
  const routes = new Map();
  const hash = crypto.createHash("sha256");
  const stat = fs.statSync(inputPath);
  const input = fs.createReadStream(inputPath, { encoding: "utf8" });
  input.on("data", (chunk) => hash.update(chunk));
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  let records = 0;
  let malformed = 0;
  let candidatePackets = 0;
  let candidateRows = 0;
  let partRows = 0;
  let partField3Values = 0;
  const protocolSession = actionTimelinePath
    ? await readProtocolJournalSession(inputPath, gameBuild)
    : null;
  const actionTimeline = actionTimelinePath
    ? await openActionTimeline(
      actionTimelinePath,
      inputPath,
      gameBuild,
      protocolSession,
    )
    : null;
  let scanCompleted = false;
  try {
    for await (const line of lines) {
      if (!line) continue;
      records += 1;
      let record;
      try {
        record = JSON.parse(line);
      } catch {
        malformed += 1;
        continue;
      }
      if (record?.data?.kind?.record !== "packet") continue;
      const packet = record.data.kind.data;
      if (packet.direction !== "client_to_server") continue;
      if (packet.route?.key?.service_id !== WORLD_SERVICE_ID) continue;
      const payload = packet.payload?.application_bytes ?? [];
      const outer = parseWireMessage(payload);
      if (!outer.valid || outer.fields.length === 0) continue;
      if (!outer.fields.every((field) => field.number === 1 && field.wire_type === 2)) continue;
      const children = outer.fields.map((field) => parseWireMessage(field.bytes));
      if (!children.every((child) => isClientHitCandidate(child))) continue;

      candidatePackets += 1;
      candidateRows += children.length;
      const routeKey = `${packet.fragment?.kind ?? "unknown"}:${WORLD_SERVICE_ID}:${packet.route.key.method_id}`;
      let route = routes.get(routeKey);
      if (!route) {
        route = {
        route_key: routeKey,
        fragment_kind: packet.fragment?.kind ?? "unknown",
        service_id: WORLD_SERVICE_ID,
        method_id: packet.route.key.method_id,
        packet_count: 0,
        client_hit_candidate_rows: 0,
        field_presence: new Map(),
        bounded_varint_values: new Map(),
        client_hit_part_candidate_rows: 0,
        part_field_3_candidate_values: 0,
        examples: [],
        decoded_action_examples: [],
        };
        routes.set(routeKey, route);
      }
      route.packet_count += 1;
      route.client_hit_candidate_rows += children.length;
      for (const [requestRowIndex, child] of children.entries()) {
        if (actionTimeline) {
          await actionTimeline.write({
            line: "action",
            data: {
              sequence: record.data.sequence,
              observed_micros: record.data.observed_micros,
              wall_clock_unix_micros: record.data.wall_clock_unix_micros,
              connection_id: packet.connection_id,
              stream_id: packet.stream_id,
              fragment_kind: packet.fragment?.kind ?? "unknown",
              service_id: WORLD_SERVICE_ID,
              method_id: packet.route.key.method_id,
              route_key: routeKey,
              request_row_index: requestRowIndex,
              ...decodeClientHitInfo(child),
            },
          });
        }
        for (const field of child.fields) {
        route.field_presence.set(field.number, (route.field_presence.get(field.number) ?? 0) + 1);
        if (field.wire_type === 0) retainBoundedValue(route.bounded_varint_values, field.number, field.value);
        if (field.number !== 13 || field.wire_type !== 2) continue;
        const part = parseWireMessage(field.bytes);
        if (!isClientHitPartCandidate(part)) continue;
        route.client_hit_part_candidate_rows += 1;
        partRows += 1;
        for (const partField of part.fields) {
          if (partField.number === 3 && partField.wire_type === 0) {
            route.part_field_3_candidate_values += 1;
            partField3Values += 1;
          }
        }
        }
      }
      if (route.examples.length < MAX_EXAMPLES_PER_ROUTE) {
        route.examples.push({
        sequence: record.data.sequence,
        observed_micros: record.data.observed_micros,
        child_rows: children.length,
        payload_bytes: payload.length,
        payload_sha256: crypto.createHash("sha256").update(Uint8Array.from(payload)).digest("hex"),
        });
      }
      if (tagAuthority && route.decoded_action_examples.length < MAX_DECODED_ACTION_EXAMPLES_PER_ROUTE) {
        for (const child of children) {
          if (route.decoded_action_examples.length >= MAX_DECODED_ACTION_EXAMPLES_PER_ROUTE) break;
          route.decoded_action_examples.push({
          sequence: record.data.sequence,
          observed_micros: record.data.observed_micros,
          ...decodeClientHitInfo(child),
          });
        }
      }
    }
    scanCompleted = true;
  } finally {
    if (actionTimeline) {
      await actionTimeline.close({
        journal_records: records,
        malformed_json_records: malformed,
        client_hit_family_candidate_packets: candidatePackets,
        client_hit_family_candidate_rows: candidateRows,
      }, scanCompleted);
    }
  }
  const actionTimelineSource = actionTimelinePath
    ? await fileReceipt(actionTimelinePath)
    : null;
  return {
    source: {
      path: inputPath.replaceAll("\\", "/"),
      bytes: stat.size,
      sha256: hash.digest("hex"),
    },
    summary: {
      journal_records: records,
      malformed_json_records: malformed,
      client_hit_family_candidate_packets: candidatePackets,
      client_hit_family_candidate_rows: candidateRows,
      distinct_candidate_routes: routes.size,
      client_hit_part_candidate_rows: partRows,
      part_field_3_candidate_values: partField3Values,
    },
    routes: [...routes.values()]
      .map((route) => finalizeRoute(route, tagAuthority))
      .sort((left, right) => right.packet_count - left.packet_count || left.route_key.localeCompare(right.route_key)),
    action_timeline_source: actionTimelineSource,
  };
}

async function readProtocolJournalSession(inputPath, gameBuild) {
  const input = fs.createReadStream(inputPath, { encoding: "utf8" });
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  for await (const line of lines) {
    if (!line) continue;
    const record = JSON.parse(line);
    assert.equal(record.line, "session", "protocol journal must begin with a session line");
    assert.equal(
      String(record.data?.game_build?.build_id),
      String(gameBuild),
      "protocol journal build does not match --game-build",
    );
    return record.data;
  }
  throw new Error("protocol journal has no session line");
}

async function openActionTimeline(
  outputPath,
  inputPath,
  gameBuild,
  protocolSession,
) {
  assert.ok(gameBuild, "action timeline requires exact game build identity");
  assert.ok(protocolSession?.capture_id, "action timeline requires source capture identity");
  const partialPath = `${outputPath}.partial`;
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  const stream = fs.createWriteStream(partialPath, { encoding: "utf8", flags: "wx" });
  await writeJsonLine(stream, {
    line: "session",
    data: {
      schema_version: ACTION_TIMELINE_SCHEMA_VERSION,
      generated_by: GENERATED_BY,
      game_build: gameBuild,
      capture_id: protocolSession.capture_id,
      source_started_unix_micros: protocolSession.started_unix_micros,
      source_protocol_pack_digest: protocolSession.protocol_pack_digest,
      source_protocol_journal: inputPath.replaceAll("\\", "/"),
      exact_protobuf_tag_authority: true,
      policy: {
        numeric_ids_are_authoritative: true,
        uint64_values_encoded_as_decimal_strings: true,
        target_allegiance_inferred: false,
        provider_ownership_inferred: false,
        remote_player_cast_packets_required: false,
        formula_authority: false,
      },
    },
  });
  let actionRows = 0;
  return {
    async write(value) {
      assert.equal(value.line, "action");
      actionRows += 1;
      await writeJsonLine(stream, value);
    },
    async close(summary, complete = true) {
      await writeJsonLine(stream, {
        line: "summary",
        data: { ...summary, emitted_action_rows: actionRows, complete },
      });
      await new Promise((resolve, reject) => {
        stream.once("error", reject);
        stream.end(resolve);
      });
      if (complete) fs.renameSync(partialPath, outputPath);
    },
  };
}

async function writeJsonLine(stream, value) {
  if (!stream.write(`${JSON.stringify(value)}\n`)) {
    await new Promise((resolve, reject) => {
      const onDrain = () => {
        stream.off("error", onError);
        resolve();
      };
      const onError = (error) => {
        stream.off("drain", onDrain);
        reject(error);
      };
      stream.once("drain", onDrain);
      stream.once("error", onError);
    });
  }
}

function isClientHitCandidate(parsed) {
  if (!parsed.valid || parsed.fields.length === 0) return false;
  if (!parsed.fields.every((field) => CLIENT_HIT_WIRE_TYPES.get(field.number) === field.wire_type)) return false;
  const present = new Set(parsed.fields.map((field) => field.number));
  return REQUIRED_CLIENT_HIT_FIELDS.every((field) => present.has(field));
}

function isClientHitPartCandidate(parsed) {
  const expected = new Map([
    [1, 0],
    [2, 2],
    [3, 0],
  ]);
  return parsed.valid && parsed.fields.length > 0 && parsed.fields.every((field) => expected.get(field.number) === field.wire_type);
}

function decodeClientHitInfo(parsed) {
  const firstVarint = (tag) => parsed.fields.find((field) => field.number === tag && field.wire_type === 0)?.value ?? null;
  return {
    source_type: firstVarint(1),
    source_or_action_uuid: firstVarint(2),
    numeric_action_id: firstVarint(3),
    action_level: firstVarint(4),
    action_stage: firstVarint(5),
    hit_event_id: firstVarint(6),
    attack_time: firstVarint(7),
    attack_uuid: firstVarint(8),
    target_uuid: firstVarint(9),
    skill_uuid: firstVarint(15),
    part_info_count: parsed.fields.filter((field) => field.number === 13 && field.wire_type === 2).length,
  };
}

function finalizeRoute(route, tagAuthority) {
  return {
    route_key: route.route_key,
    fragment_kind: route.fragment_kind,
    service_id: route.service_id,
    method_id: route.method_id,
    packet_count: route.packet_count,
    client_hit_candidate_rows: route.client_hit_candidate_rows,
    ordinal_field_presence: Object.fromEntries([...route.field_presence].sort((left, right) => left[0] - right[0])),
    bounded_ordinal_varint_values: Object.fromEntries(
      [...route.bounded_varint_values]
        .sort((left, right) => left[0] - right[0])
        .map(([field, values]) => [field, [...values].sort(compareIntegerStrings)]),
    ),
    client_hit_part_candidate_rows: route.client_hit_part_candidate_rows,
    part_field_3_candidate_values: route.part_field_3_candidate_values,
    examples: route.examples,
    decoded_action_examples: route.decoded_action_examples,
    assigned_proxy_method_name: null,
    protobuf_tag_identity_proven: tagAuthority,
    formula_authority: false,
  };
}

function retainBoundedValue(valuesByField, field, value) {
  let values = valuesByField.get(field);
  if (!values) {
    values = new Set();
    valuesByField.set(field, values);
  }
  if (values.size < MAX_VALUES_PER_FIELD) values.add(value);
}

function compareIntegerStrings(left, right) {
  const a = BigInt(left);
  const b = BigInt(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

function parseWireMessage(bytes) {
  const fields = [];
  let offset = 0;
  while (offset < bytes.length && fields.length < 256) {
    const key = readVarint(bytes, offset);
    if (!key) return { valid: false, fields };
    offset = key.next;
    const number = Number(key.value >> 3n);
    const wireType = Number(key.value & 7n);
    if (number < 1 || wireType === 3 || wireType === 4 || wireType > 5) return { valid: false, fields };
    const field = { number, wire_type: wireType };
    if (wireType === 0) {
      const value = readVarint(bytes, offset);
      if (!value) return { valid: false, fields };
      field.value = value.value.toString();
      offset = value.next;
    } else if (wireType === 1) {
      if (offset + 8 > bytes.length) return { valid: false, fields };
      offset += 8;
    } else if (wireType === 2) {
      const length = readVarint(bytes, offset);
      if (!length || length.value > BigInt(Number.MAX_SAFE_INTEGER)) return { valid: false, fields };
      offset = length.next;
      const end = offset + Number(length.value);
      if (end > bytes.length) return { valid: false, fields };
      field.bytes = bytes.slice(offset, end);
      offset = end;
    } else if (wireType === 5) {
      if (offset + 4 > bytes.length) return { valid: false, fields };
      offset += 4;
    }
    fields.push(field);
  }
  return { valid: offset === bytes.length, fields };
}

function readVarint(bytes, start) {
  let value = 0n;
  for (let index = 0; index < 10 && start + index < bytes.length; index += 1) {
    const byte = bytes[start + index];
    value |= BigInt(byte & 0x7f) << BigInt(index * 7);
    if ((byte & 0x80) === 0) return { value, next: start + index + 1 };
  }
  return null;
}

async function fileReceipt(inputPath) {
  const stat = fs.statSync(inputPath);
  const hash = crypto.createHash("sha256");
  for await (const chunk of fs.createReadStream(inputPath)) hash.update(chunk);
  return { path: inputPath.replaceAll("\\", "/"), bytes: stat.size, sha256: hash.digest("hex") };
}

function verify(values) {
  const inputPath = path.resolve(required(values, "input"));
  const report = JSON.parse(fs.readFileSync(inputPath, "utf8"));
  verifyReport(report);
  console.log(`verified ${inputPath}`);
}

function verifyReport(report) {
  assert.ok(SUPPORTED_REPORT_SCHEMA_VERSIONS.has(report.schema_version));
  assert.equal(report.generated_by, GENERATED_BY);
  assert.ok(report.summary.client_hit_family_candidate_packets > 0);
  assert.equal(report.summary.distinct_candidate_routes, report.client_hit_family_candidate_routes.length);
  assert.ok(report.client_hit_family_candidate_routes.every((route) => route.assigned_proxy_method_name === null));
  assert.ok(
    report.client_hit_family_candidate_routes.every(
      (route) => route.protobuf_tag_identity_proven === report.conclusion.client_hit_part_damage_value_tag_proven,
    ),
  );
  assert.equal(report.conclusion.individual_sync_hit_method_mapping_proven, false);
  assert.equal(report.conclusion.exact_server_relation_to_part_field_3_proven, false);
  assert.equal(report.conclusion.runtime_promotion_allowed, false);
  assert.equal(report.conclusion.provider_rdps_credit_allowed, false);
  if (report.schema_version >= 3) {
    assert.equal(
      report.conclusion.complete_local_action_timeline_emitted,
      report.sources.local_action_timeline !== null,
    );
    assert.equal(
      report.policy.complete_decoded_local_action_rows_streamed,
      report.sources.local_action_timeline !== null,
    );
    assert.equal(report.policy.local_action_timeline_infers_actor_allegiance, false);
    assert.equal(report.policy.local_action_timeline_infers_provider_ownership, false);
  }
  assert.equal(report.content_sha256, contentSha256(report));
}

async function selfTest() {
  const child = [8, 1, 16, 2, 24, 3, 64, 4, 72, 5, 82, 0, 90, 0, 98, 0];
  const outer = [10, child.length, ...child];
  const parsed = parseWireMessage(outer);
  assert.equal(parsed.valid, true);
  assert.equal(isClientHitCandidate(parseWireMessage(parsed.fields[0].bytes)), true);
  assert.equal(isClientHitPartCandidate(parseWireMessage([8, 1, 18, 0, 24, 9])), true);
  assert.equal(isClientHitPartCandidate(parseWireMessage([32, 1])), false);
  assert.deepEqual(decodeClientHitInfo(parseWireMessage(child)), {
    source_type: "1",
    source_or_action_uuid: "2",
    numeric_action_id: "3",
    action_level: null,
    action_stage: null,
    hit_event_id: null,
    attack_time: null,
    attack_uuid: "4",
    target_uuid: "5",
    skill_uuid: null,
    part_info_count: 0,
  });
  const tempDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "rlogs-client-hit-actions-"));
  const timelinePath = path.join(tempDirectory, "actions.jsonl");
  try {
    const timeline = await openActionTimeline(
      timelinePath,
      path.join(tempDirectory, "source.protocol.jsonl"),
      "test-build",
      {
        capture_id: "test-capture",
        started_unix_micros: 123,
        protocol_pack_digest: "sha256:test",
      },
    );
    await timeline.write({
      line: "action",
      data: {
        sequence: 1,
        source_or_action_uuid: "9007199254740993",
        target_uuid: "9007199254740995",
        attack_uuid: "9007199254740997",
      },
    });
    await timeline.close({
      journal_records: 1,
      malformed_json_records: 0,
      client_hit_family_candidate_packets: 1,
      client_hit_family_candidate_rows: 1,
    });
    const timelineLines = fs.readFileSync(timelinePath, "utf8").trim().split("\n").map(JSON.parse);
    assert.equal(timelineLines[0].line, "session");
    assert.equal(timelineLines[0].data.capture_id, "test-capture");
    assert.equal(timelineLines[1].data.source_or_action_uuid, "9007199254740993");
    assert.equal(timelineLines[2].data.emitted_action_rows, 1);
    assert.equal(fs.existsSync(`${timelinePath}.partial`), false);
  } finally {
    fs.rmSync(tempDirectory, { recursive: true, force: true });
  }
  console.log("self-test passed");
}

function contentSha256(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(JSON.stringify(copy)).digest("hex");
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
      "  node tools/bpsr-client-hit-wire-candidate.mjs build --journal FILE --il2cpp-dump FILE --game-build BUILD [--native-wire-proof FILE] [--actions-output FILE] --output FILE\n" +
      "  node tools/bpsr-client-hit-wire-candidate.mjs verify --input FILE\n" +
      "  node tools/bpsr-client-hit-wire-candidate.mjs self-test",
  );
  process.exit(exitCode);
}
