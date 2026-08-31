#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createReadStream, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-autoattack-client-journal-census.mjs";
const SCHEMA_VERSION = 1;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseOptions(rest);
if (command === "generate") await generate(options);
else if (command === "verify") await verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function generate(values) {
  const journalDir = path.resolve(required(values, "journal-dir"));
  const build = required(values, "build");
  const abilityId = Number(required(values, "ability-id"));
  const output = path.resolve(required(values, "output"));
  assert.ok(Number.isSafeInteger(abilityId) && abilityId > 0, "invalid ability ID");
  assert.throws(() => statSync(output), { code: "ENOENT" }, `refusing to overwrite ${output}`);

  const names = readdirSync(journalDir)
    .filter((name) => name.endsWith(`-steam-${build}.protocol.jsonl`))
    .sort();
  assert.ok(names.length > 0, "no exact-build journal filenames found");
  const varint = encodeVarint(abilityId);
  const journals = [];
  let totalBytes = 0;
  let totalLines = 0;
  let clientPackets = 0;
  let clientPayloadBytes = 0;
  let clientPayloadsWithAbility = 0;
  let abilityVarintOccurrences = 0;
  let parseErrors = 0;
  let maximumLineBytes = 0;
  const routeCounts = new Map();
  const protocolPackDigests = new Set();

  for (const name of names) {
    const filePath = path.join(journalDir, name);
    const result = await scanJournal(filePath, build, varint);
    journals.push(result);
    totalBytes += result.bytes;
    totalLines += result.line_count;
    clientPackets += result.client_packet_count;
    clientPayloadBytes += result.client_application_bytes;
    clientPayloadsWithAbility += result.client_payloads_with_ability_varint;
    abilityVarintOccurrences += result.ability_varint_occurrences;
    parseErrors += result.parse_errors;
    maximumLineBytes = Math.max(maximumLineBytes, result.maximum_line_bytes);
    if (result.protocol_pack_digest) protocolPackDigests.add(result.protocol_pack_digest);
    for (const [route, count] of Object.entries(result.client_route_counts)) {
      routeCounts.set(route, (routeCounts.get(route) ?? 0) + count);
    }
  }

  const nonempty = journals.filter((entry) => entry.bytes > 0);
  const empty = journals.filter((entry) => entry.bytes === 0);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: build,
    identity: {
      ability_id: abilityId,
      ability_id_varint: varint,
      scan_scope: `direct files matching *-steam-${build}.protocol.jsonl`,
    },
    policy: {
      exact_numeric_ability_id_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      scan_is_streaming_and_one_journal_at_a_time: true,
      server_to_client_occurrences_count_as_local_requests: false,
      raw_varint_presence_alone_proves_client_hit_message_identity: false,
      missing_client_varint_proves_remote_cast_absence: false,
      missing_client_varint_proves_ability_absence: false,
      empty_journals_are_preserved_and_not_treated_as_negative_packets: true,
      remote_player_cast_packets_required: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
    source_directory: journalDir,
    protocol_pack_digests: [...protocolPackDigests].sort(),
    summary: {
      journal_files: journals.length,
      nonempty_exact_build_journals: nonempty.length,
      empty_journals: empty.length,
      total_bytes: totalBytes,
      total_lines: totalLines,
      client_packet_records: clientPackets,
      client_application_bytes: clientPayloadBytes,
      client_payloads_with_ability_varint: clientPayloadsWithAbility,
      ability_varint_occurrences: abilityVarintOccurrences,
      client_route_counts: Object.fromEntries([...routeCounts].sort()),
      parse_errors: parseErrors,
      maximum_line_bytes: maximumLineBytes,
      peak_working_set_bytes: process.memoryUsage().rss,
    },
    journals,
    conclusion: {
      all_nonempty_journal_headers_match_exact_build: nonempty.every(
        (entry) => entry.session_build === build,
      ),
      all_retained_client_packet_records_scanned: parseErrors === 0,
      ability_varint_observed_in_client_payload: clientPayloadsWithAbility > 0,
      exact_client_hit_request_for_ability_observed: false,
      exact_server_operator_or_rounding_proven: false,
      retained_raw_journal_frontier_exhausted: parseErrors === 0,
      smallest_next_proof:
        "Acquire one controlled local ability-2900840 capture that retains its client request and exact SyncDamageInfo response, or recover an authoritative server AutoAttack operator. The retained raw-journal set contains no client payload with the numeric ability varint; unavailable remote-player cast packets remain unnecessary.",
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
  };
  assert.equal(report.conclusion.ability_varint_observed_in_client_payload, false);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(
    `wrote ${output}: ${journals.length} journals, ${clientPackets} client packets, ` +
    `${clientPayloadsWithAbility} payloads with ability ${abilityId}, peak RSS ${report.summary.peak_working_set_bytes}`,
  );
}

async function scanJournal(filePath, build, abilityVarint) {
  const stat = statSync(filePath);
  const hash = createHash("sha256");
  const routeCounts = new Map();
  let carry = "";
  let lineCount = 0;
  let maximumLineBytes = 0;
  let clientPacketCount = 0;
  let clientApplicationBytes = 0;
  let clientPayloadsWithAbility = 0;
  let abilityVarintOccurrences = 0;
  let parseErrors = 0;
  let sessionBuild = null;
  let protocolPackDigest = null;

  const handleLine = (line) => {
    lineCount += 1;
    maximumLineBytes = Math.max(maximumLineBytes, Buffer.byteLength(line));
    if (lineCount === 1) {
      try {
        const row = JSON.parse(line);
        assert.equal(row.line, "session");
        sessionBuild = String(row.data?.game_build?.build_id ?? "");
        protocolPackDigest = row.data?.protocol_pack_digest ?? null;
        assert.equal(sessionBuild, build, `journal build mismatch: ${filePath}`);
      } catch (error) {
        parseErrors += 1;
        throw error;
      }
      return;
    }
    if (!line.includes('"direction":"client_to_server"')) return;
    try {
      const row = JSON.parse(line);
      const packet = row.data?.kind?.record === "packet" ? row.data.kind.data : null;
      if (!packet || packet.direction !== "client_to_server") return;
      clientPacketCount += 1;
      const route = packet.route?.key ?? {};
      const routeKey = `${route.fragment?.kind ?? "unknown"}:${route.service_id ?? "null"}:${route.method_id ?? "null"}`;
      routeCounts.set(routeKey, (routeCounts.get(routeKey) ?? 0) + 1);
      const payload = packet.payload?.application_bytes ?? [];
      clientApplicationBytes += payload.length;
      const occurrences = countSubsequence(payload, abilityVarint);
      if (occurrences > 0) clientPayloadsWithAbility += 1;
      abilityVarintOccurrences += occurrences;
    } catch (error) {
      parseErrors += 1;
      throw error;
    }
  };

  for await (const chunk of createReadStream(filePath, { encoding: "utf8" })) {
    hash.update(chunk, "utf8");
    carry += chunk;
    let newline;
    while ((newline = carry.indexOf("\n")) >= 0) {
      const line = carry.slice(0, newline).replace(/\r$/, "");
      carry = carry.slice(newline + 1);
      if (line) handleLine(line);
    }
  }
  if (carry) handleLine(carry.replace(/\r$/, ""));
  if (stat.size === 0) {
    assert.equal(lineCount, 0);
    assert.equal(sessionBuild, null);
  } else {
    assert.ok(lineCount > 0);
    assert.equal(sessionBuild, build);
  }
  return {
    path: path.resolve(filePath),
    bytes: stat.size,
    sha256: hash.digest("hex"),
    line_count: lineCount,
    maximum_line_bytes: maximumLineBytes,
    session_build: sessionBuild,
    protocol_pack_digest: protocolPackDigest,
    client_packet_count: clientPacketCount,
    client_application_bytes: clientApplicationBytes,
    client_payloads_with_ability_varint: clientPayloadsWithAbility,
    ability_varint_occurrences: abilityVarintOccurrences,
    client_route_counts: Object.fromEntries([...routeCounts].sort()),
    parse_errors: parseErrors,
  };
}

async function verify(values) {
  const report = JSON.parse(readFileSync(path.resolve(required(values, "input")), "utf8"));
  verifyReport(report);
  for (const entry of report.journals) await verifyJournalReceipt(entry);
  console.log(`verified ${path.resolve(required(values, "input"))}`);
}

function verifyReport(report) {
  assert.equal(report.schema_version, SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.policy?.scan_is_streaming_and_one_journal_at_a_time, true);
  assert.equal(report.policy?.remote_player_cast_packets_required, false);
  assert.equal(report.policy?.provider_rdps_credit_allowed, false);
  assert.equal(report.summary?.journal_files, report.journals?.length);
  assert.equal(report.summary?.parse_errors, 0);
  assert.ok(report.summary?.client_packet_records > 0);
  assert.equal(report.summary?.client_payloads_with_ability_varint, 0);
  assert.equal(report.summary?.ability_varint_occurrences, 0);
  assert.equal(report.conclusion?.all_nonempty_journal_headers_match_exact_build, true);
  assert.equal(report.conclusion?.all_retained_client_packet_records_scanned, true);
  assert.equal(report.conclusion?.ability_varint_observed_in_client_payload, false);
  assert.equal(report.conclusion?.exact_client_hit_request_for_ability_observed, false);
  assert.equal(report.conclusion?.exact_server_operator_or_rounding_proven, false);
  assert.equal(report.conclusion?.provider_rdps_credit_allowed, false);
  assert.equal(report.conclusion?.runtime_promotion_allowed, false);
  assert.equal(report.content_sha256, contentHash(withoutContentHash(report)));
}

async function verifyJournalReceipt(entry) {
  const absolute = path.resolve(entry.path);
  assert.equal(statSync(absolute).size, entry.bytes, `journal bytes changed: ${entry.path}`);
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(absolute)) hash.update(chunk);
  assert.equal(hash.digest("hex"), entry.sha256, `journal hash changed: ${entry.path}`);
}

function countSubsequence(values, needle) {
  let count = 0;
  for (let index = 0; index <= values.length - needle.length; index += 1) {
    let matches = true;
    for (let offset = 0; offset < needle.length; offset += 1) {
      if (values[index + offset] !== needle[offset]) {
        matches = false;
        break;
      }
    }
    if (matches) count += 1;
  }
  return count;
}

function encodeVarint(value) {
  const bytes = [];
  let remaining = value;
  while (remaining >= 0x80) {
    bytes.push((remaining & 0x7f) | 0x80);
    remaining = Math.floor(remaining / 0x80);
  }
  bytes.push(remaining);
  return bytes;
}

function selfTest() {
  assert.deepEqual(encodeVarint(2_900_840), [232, 134, 177, 1]);
  assert.equal(countSubsequence([1, 232, 134, 177, 1, 2], [232, 134, 177, 1]), 1);
  assert.equal(countSubsequence([232, 134, 177], [232, 134, 177, 1]), 0);
  console.log("bpsr-autoattack-client-journal-census self-test passed");
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function contentHash(value) {
  return sha256(Buffer.from(JSON.stringify(canonicalize(withoutContentHash(value))), "utf8"));
}

function withoutContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return copy;
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonicalize(value[key])]));
  }
  return value;
}

function parseOptions(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    assert.ok(flag?.startsWith("--") && value, `invalid option near ${flag ?? "end"}`);
    parsed[flag.slice(2)] = value;
  }
  return parsed;
}

function required(values, name) {
  const value = values[name];
  assert.ok(value, `--${name} is required`);
  return value;
}

function usage(code) {
  console.log(
    "Usage:\n" +
    "  node tools/bpsr-autoattack-client-journal-census.mjs generate --journal-dir <dir> " +
    "--build <id> --ability-id <id> --output <json>\n" +
    "  node tools/bpsr-autoattack-client-journal-census.mjs verify --input <json>\n" +
    "  node tools/bpsr-autoattack-client-journal-census.mjs self-test",
  );
  process.exit(code);
}
