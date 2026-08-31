#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";

const GENERATED_BY = "tools/bpsr-client-packet-route-inventory.mjs";
const SCHEMA_VERSION = 3;
const SUPPORTED_SCHEMA_VERSIONS = new Set([2, 3]);
const MAX_SIGNATURES_PER_ROUTE = 32;
const MAX_EXAMPLES_PER_ROUTE = 3;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") await build(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function build(values) {
  const journalValue = values.get("journal");
  const journalDirValue = values.get("journal-dir");
  assert.notEqual(Boolean(journalValue), Boolean(journalDirValue), "choose exactly one of --journal or --journal-dir");
  const journalDir = path.resolve(journalDirValue ?? path.dirname(required(values, "journal")));
  const gameBuild = required(values, "game-build");
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const suffix = `-steam-${gameBuild}.protocol.jsonl`;
  const journalPaths = journalValue
    ? [path.resolve(journalValue)]
    : fs
        .readdirSync(journalDir, { withFileTypes: true })
        .filter((entry) => entry.isFile() && entry.name.endsWith(suffix))
        .map((entry) => path.join(journalDir, entry.name))
        .sort();
  assert.ok(journalPaths.length > 0, `no journals ending in ${suffix}`);

  const routes = new Map();
  const sources = [];
  let totalRecords = 0;
  let clientPackets = 0;
  let serverPackets = 0;
  for (const journalPath of journalPaths) {
    const receipt = await scanJournal(journalPath, routes, () => {
      totalRecords += 1;
    }, (direction) => {
      if (direction === "client_to_server") clientPackets += 1;
      else if (direction === "server_to_client") serverPackets += 1;
    });
    sources.push(receipt);
  }

  const routeRows = [...routes.values()]
    .map(finalizeRoute)
    .sort((left, right) => right.packet_count - left.packet_count || left.route_key.localeCompare(right.route_key));
  const output = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    source_directory: journalDir.replaceAll("\\", "/"),
    source_selection: journalValue ? "explicit_single_journal" : "exact_build_directory_suffix",
    policy: {
      streaming_one_journal_at_a_time: true,
      raw_payloads_copied_to_report: false,
      unknown_client_routes_preserved: true,
      protobuf_field_names_inferred: false,
      route_names_inferred: false,
      remote_player_cast_packets_required: false,
      formula_authority: false,
      runtime_promotion_allowed: false,
      provider_rdps_credit_allowed: false,
    },
    summary: {
      journal_files: sources.length,
      total_source_bytes: sources.reduce((sum, source) => sum + source.bytes, 0),
      total_records: totalRecords,
      malformed_json_records: sources.reduce((sum, source) => sum + source.malformed_json_records, 0),
      trailing_truncated_json_records: sources.reduce((sum, source) => sum + source.trailing_truncated_json_records, 0),
      malformed_json_records_before_later_valid_record: sources.reduce((sum, source) => sum + source.malformed_json_records_before_later_valid_record, 0),
      client_to_server_packets: clientPackets,
      server_to_client_packets: serverPackets,
      distinct_client_routes: routeRows.length,
      routes_with_valid_top_level_protobuf: routeRows.filter((route) => route.valid_top_level_protobuf_packets > 0).length,
      routes_with_valid_nested_field_1_protobuf: routeRows.filter((route) => route.valid_nested_field_1_protobuf_packets > 0).length,
    },
    sources,
    client_routes: routeRows,
    conclusion: {
      exact_client_hit_route_proven: false,
      reason: "This receipt inventories exact wire route IDs and bounded protobuf shapes only. It does not assign generated message names without a matching exact-build route or controlled capture proof.",
      smallest_next_proof: "Correlate one locally performed direct damage skill with a newly appearing or time-aligned client route, then decode its ClientHitInfo fields and compare ClientHitPartInfo.DamageVal with the exact server SyncDamageInfo response.",
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

async function scanJournal(journalPath, routes, onRecord, onPacket) {
  const stat = fs.statSync(journalPath);
  const hash = crypto.createHash("sha256");
  const input = fs.createReadStream(journalPath, { encoding: "utf8" });
  input.on("data", (chunk) => hash.update(chunk));
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  let records = 0;
  let clientPackets = 0;
  let malformedJsonRecords = 0;
  let pendingMalformedJsonRecords = 0;
  let malformedJsonRecordsBeforeLaterValidRecord = 0;
  for await (const line of lines) {
    if (!line) continue;
    records += 1;
    onRecord();
    let record;
    try {
      record = JSON.parse(line);
    } catch {
      malformedJsonRecords += 1;
      pendingMalformedJsonRecords += 1;
      continue;
    }
    malformedJsonRecordsBeforeLaterValidRecord += pendingMalformedJsonRecords;
    pendingMalformedJsonRecords = 0;
    if (record?.data?.kind?.record !== "packet") continue;
    const packet = record.data.kind.data;
    onPacket(packet.direction);
    if (packet.direction !== "client_to_server") continue;
    clientPackets += 1;
    observeClientPacket(routes, path.basename(journalPath), record.data, packet);
  }
  return {
    path: journalPath.replaceAll("\\", "/"),
    bytes: stat.size,
    sha256: hash.digest("hex"),
    records,
    malformed_json_records: malformedJsonRecords,
    trailing_truncated_json_records: pendingMalformedJsonRecords,
    malformed_json_records_before_later_valid_record: malformedJsonRecordsBeforeLaterValidRecord,
    client_to_server_packets: clientPackets,
  };
}

function observeClientPacket(routes, journalName, envelope, packet) {
  const fragment = packet.fragment?.kind ?? "unknown";
  const serviceId = packet.route?.key?.service_id ?? null;
  const methodId = packet.route?.key?.method_id ?? null;
  const routeKey = `${fragment}:${serviceId}:${methodId}`;
  let row = routes.get(routeKey);
  if (!row) {
    row = {
      route_key: routeKey,
      fragment_kind: fragment,
      service_id: serviceId,
      method_id: methodId,
      packet_count: 0,
      journals: new Set(),
      payload_bytes_min: null,
      payload_bytes_max: null,
      payload_bytes_total: 0,
      valid_top_level_protobuf_packets: 0,
      valid_nested_field_1_protobuf_packets: 0,
      signatures: new Map(),
      signature_overflow_packets: 0,
      examples: [],
    };
    routes.set(routeKey, row);
  }
  const payload = packet.payload?.application_bytes ?? [];
  row.packet_count += 1;
  row.journals.add(journalName);
  row.payload_bytes_min = row.payload_bytes_min === null ? payload.length : Math.min(row.payload_bytes_min, payload.length);
  row.payload_bytes_max = row.payload_bytes_max === null ? payload.length : Math.max(row.payload_bytes_max, payload.length);
  row.payload_bytes_total += payload.length;

  const parsed = parseWireMessage(payload);
  const topSignature = parsed.valid ? signature(parsed.fields) : "<invalid>";
  if (parsed.valid) row.valid_top_level_protobuf_packets += 1;
  let nestedField1Signature = null;
  if (parsed.valid) {
    const field1 = parsed.fields.find((field) => field.number === 1 && field.wire_type === 2);
    if (field1) {
      const nested = parseWireMessage(field1.bytes);
      if (nested.valid) {
        row.valid_nested_field_1_protobuf_packets += 1;
        nestedField1Signature = signature(nested.fields);
      }
    }
  }
  const combined = nestedField1Signature ? `${topSignature} -> field1:${nestedField1Signature}` : topSignature;
  incrementBounded(row.signatures, combined, row);
  if (row.examples.length < MAX_EXAMPLES_PER_ROUTE) {
    row.examples.push({
      journal: journalName,
      sequence: envelope.sequence,
      observed_micros: envelope.observed_micros,
      payload_bytes: payload.length,
      payload_sha256: crypto.createHash("sha256").update(Uint8Array.from(payload)).digest("hex"),
      protobuf_shape: combined,
    });
  }
}

function parseWireMessage(bytes) {
  const fields = [];
  let offset = 0;
  while (offset < bytes.length && fields.length < 128) {
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

function signature(fields) {
  return fields.map((field) => `${field.number}:${field.wire_type}`).join(",");
}

function incrementBounded(counter, key, row) {
  if (counter.has(key)) counter.set(key, counter.get(key) + 1);
  else if (counter.size < MAX_SIGNATURES_PER_ROUTE) counter.set(key, 1);
  else row.signature_overflow_packets += 1;
}

function finalizeRoute(row) {
  return {
    route_key: row.route_key,
    fragment_kind: row.fragment_kind,
    service_id: row.service_id,
    method_id: row.method_id,
    packet_count: row.packet_count,
    journal_count: row.journals.size,
    journals: [...row.journals].sort(),
    payload_bytes_min: row.payload_bytes_min,
    payload_bytes_max: row.payload_bytes_max,
    payload_bytes_mean: row.packet_count === 0 ? null : row.payload_bytes_total / row.packet_count,
    valid_top_level_protobuf_packets: row.valid_top_level_protobuf_packets,
    valid_nested_field_1_protobuf_packets: row.valid_nested_field_1_protobuf_packets,
    bounded_protobuf_shapes: Object.fromEntries([...row.signatures].sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))),
    protobuf_shape_overflow_packets: row.signature_overflow_packets,
    examples: row.examples,
    assigned_message_name: null,
    exact_client_hit_route_proven: false,
  };
}

function verify(values) {
  const inputPath = path.resolve(required(values, "input"));
  const report = JSON.parse(fs.readFileSync(inputPath, "utf8"));
  verifyReport(report);
  console.log(`verified ${inputPath}`);
}

function verifyReport(report) {
  assert.ok(SUPPORTED_SCHEMA_VERSIONS.has(report.schema_version));
  assert.equal(report.generated_by, GENERATED_BY);
  assert.ok(report.summary.journal_files > 0);
  assert.equal(report.summary.client_to_server_packets, report.client_routes.reduce((sum, route) => sum + route.packet_count, 0));
  assert.ok(report.client_routes.every((route) => route.assigned_message_name === null));
  assert.equal(report.conclusion.exact_client_hit_route_proven, false);
  assert.equal(report.conclusion.runtime_promotion_allowed, false);
  assert.equal(report.conclusion.provider_rdps_credit_allowed, false);
  assert.equal(report.content_sha256, contentSha256(report));
}

function selfTest() {
  const bytes = [10, 3, 8, 150, 1, 16, 7];
  const parsed = parseWireMessage(bytes);
  assert.equal(parsed.valid, true);
  assert.equal(signature(parsed.fields), "1:2,2:0");
  const nested = parseWireMessage(parsed.fields[0].bytes);
  assert.equal(nested.valid, true);
  assert.equal(signature(nested.fields), "1:0");
  assert.equal(nested.fields[0].value, "150");
  assert.equal(parseWireMessage([10, 5, 1]).valid, false);
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
      "  node tools/bpsr-client-packet-route-inventory.mjs build (--journal FILE | --journal-dir DIR) --game-build BUILD --output FILE\n" +
      "  node tools/bpsr-client-packet-route-inventory.mjs verify --input FILE\n" +
      "  node tools/bpsr-client-packet-route-inventory.mjs self-test",
  );
  process.exit(exitCode);
}
