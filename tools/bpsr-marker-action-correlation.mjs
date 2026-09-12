#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-marker-action-correlation.mjs";
const SCHEMA_VERSION = 1;
const DEFAULT_BASELINE_MICROS = 3_000_000;
const DEFAULT_RESPONSE_MICROS = 3_000_000;
const MAX_RECORDS = 5_000_000;

const [command = "help", ...argv] = process.argv.slice(2);
const options = parseArgs(argv);

if (command === "analyze") analyzeCommand(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyzeCommand(values) {
  const actionsPath = requiredPath(values, "actions");
  const sessionPath = requiredPath(values, "session");
  const journalPath = requiredPath(values, "journal");
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const result = analyze({
    actionsPath,
    sessionPath,
    journalPath,
    expectedActionsSha256: optionalSha(values["actions-sha256"], "--actions-sha256"),
    expectedSessionSha256: optionalSha(values["session-sha256"], "--session-sha256"),
    expectedJournalSha256: optionalSha(values["journal-sha256"], "--journal-sha256"),
    baselineMicros: parsePositive(values["baseline-ms"], DEFAULT_BASELINE_MICROS / 1000, "--baseline-ms") * 1000,
    responseMicros: parsePositive(values["response-ms"], DEFAULT_RESPONSE_MICROS / 1000, "--response-ms") * 1000,
  });
  result.content_sha256 = contentHash(result);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  const partial = `${outputPath}.partial`;
  fs.writeFileSync(partial, `${JSON.stringify(result, null, 2)}\n`, { flag: "wx" });
  fs.renameSync(partial, outputPath);
  process.stdout.write(`${JSON.stringify(result.summary, null, 2)}\n`);
}

function verifyCommand(values) {
  const reportPath = requiredPath(values, "report");
  const report = readJson(reportPath, "correlation report");
  validateReportEnvelope(report);
  const actualContentHash = contentHash(report);
  if (actualContentHash !== report.content_sha256) throw new Error("Correlation report content hash mismatch");
  for (const [key, descriptor] of Object.entries(report.inputs)) {
    requireFile(descriptor.path, `${key} input`);
    const actual = sha256File(descriptor.path);
    if (actual !== descriptor.sha256) throw new Error(`${key} input hash mismatch`);
  }
  process.stdout.write("marker correlation report verified\n");
}

function analyze({ actionsPath, sessionPath, journalPath, expectedActionsSha256, expectedSessionSha256, expectedJournalSha256, baselineMicros, responseMicros }) {
  const hashes = {
    actions: sha256File(actionsPath),
    session: sha256File(sessionPath),
    journal: sha256File(journalPath),
  };
  verifyExpectedHash(hashes.actions, expectedActionsSha256, "action ledger");
  verifyExpectedHash(hashes.session, expectedSessionSha256, "session manifest");
  verifyExpectedHash(hashes.journal, expectedJournalSha256, "protocol journal");

  const actions = readJson(actionsPath, "marker action ledger");
  const session = readJson(sessionPath, "marker capture session");
  const journal = readJournal(journalPath);
  validateEvidence(actions, session, journal.session, hashes, journal.records);

  const packets = normalizePackets(journal.records);
  const gaps = normalizeGaps(journal.records);
  const windows = actions.actions.map((action, index) => buildWindow(
    action,
    index,
    actions.actions,
    actions.action_flow_windows ?? [],
    packets,
    gaps,
    baselineMicros,
    responseMicros,
  ));
  const actionPayloadPresence = new Map();
  for (const window of windows) {
    for (const packet of window.outbound) {
      const set = actionPayloadPresence.get(packet.payload_sha256) ?? new Set();
      set.add(window.marker_number);
      actionPayloadPresence.set(packet.payload_sha256, set);
    }
  }

  let candidateCount = 0;
  let gapAffectedWindowCount = 0;
  const markerWindows = windows.map((window) => {
    const baselineRouteCounts = counts(window.baseline.map((packet) => packet.route_signature));
    const baselinePayloadCounts = counts(window.baseline.map((packet) => packet.payload_sha256));
    const inboundBaselineRouteCounts = counts(window.baselineInbound.map((packet) => packet.route_signature));
    const inboundBaselinePayloadCounts = counts(window.baselineInbound.map((packet) => packet.payload_sha256));
    const candidates = window.outbound.map((packet) => {
      const samePayloadMarkers = [...(actionPayloadPresence.get(packet.payload_sha256) ?? [])].sort((a, b) => a - b);
      const correlatedInbound = window.inbound
        .filter((candidate) => candidate.wall_clock_unix_micros >= packet.wall_clock_unix_micros && candidate.connection_id === packet.connection_id)
        .map((candidate) => ({
          ...publicPacket(candidate),
          correlation_basis: returnCorrelationBasis(packet, candidate),
          route_seen_in_baseline: Boolean(inboundBaselineRouteCounts[candidate.route_signature]),
          payload_seen_in_baseline: Boolean(inboundBaselinePayloadCounts[candidate.payload_sha256]),
        }));
      const routeSeen = Boolean(baselineRouteCounts[packet.route_signature]);
      const payloadSeen = Boolean(baselinePayloadCounts[packet.payload_sha256]);
      const strongestReturn = correlatedInbound.find((candidate) => candidate.correlation_basis === "matching-call-id") ??
        correlatedInbound.find((candidate) => candidate.fragment_kind === "return") ?? null;
      let score = (payloadSeen ? 0 : 4) + (routeSeen ? 0 : 3) + (samePayloadMarkers.length === 1 ? 2 : 0);
      if (strongestReturn) score += 2;
      if (correlatedInbound.some((candidate) => !candidate.route_seen_in_baseline || !candidate.payload_seen_in_baseline)) score += 1;
      if (window.gaps.length > 0) score -= 5;
      candidateCount += 1;
      return {
        ...publicPacket(packet),
        route_seen_in_baseline: routeSeen,
        payload_seen_in_baseline: payloadSeen,
        payload_seen_in_marker_windows: samePayloadMarkers,
        candidate_score: score,
        correlated_server_return: strongestReturn,
        correlated_inbound_changes: correlatedInbound.filter((candidate) => !candidate.route_seen_in_baseline || !candidate.payload_seen_in_baseline),
        correlation_is_protocol_proof: false,
      };
    }).sort((a, b) => b.candidate_score - a.candidate_score || a.wall_clock_unix_micros - b.wall_clock_unix_micros || a.sequence - b.sequence);
    if (window.gaps.length > 0) gapAffectedWindowCount += 1;
    return {
      marker_number: window.marker_number,
      expected_action: window.expected_action,
      placement_ready_unix_micros: window.start,
      placement_completed_unix_micros: window.completed,
      response_window_end_unix_micros: window.end,
      baseline_start_unix_micros: window.baselineStart,
      baseline_packet_count: window.baseline.length + window.baselineInbound.length,
      outbound_candidate_count: candidates.length,
      integrity: window.gaps.length === 0 ? "complete-in-journal" : "gap-observed",
      gaps: window.gaps,
      candidates,
    };
  });

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    generated_utc: new Date().toISOString(),
    policy: {
      mode: "offline-read-only",
      transmits_or_injects_traffic: false,
      candidate_meaning: "timestamp correlation and baseline novelty only; never protocol or placement proof",
      duplicate_policy: "exact duplicate packet records are collapsed; frame containers are omitted when decoded child records share their timestamp, direction, and connection",
      fail_closed_on: ["identity mismatch", "hash mismatch", "timestamp disorder", "action outside capture", "journal sequence disorder"],
    },
    capture: {
      capture_id: session.capture_id,
      scene: session.scene,
      game_build: actions.game_build,
      protocol_pack_digest: actions.protocol_pack_digest,
      journal_record_count: journal.records.length,
      normalized_packet_count: packets.length,
      journal_gap_count: gaps.length,
    },
    inputs: {
      actions: { path: actionsPath, sha256: hashes.actions },
      session: { path: sessionPath, sha256: hashes.session },
      journal: { path: journalPath, sha256: hashes.journal },
    },
    analysis: { baseline_micros: baselineMicros, response_micros: responseMicros },
    marker_windows: markerWindows,
    summary: {
      marker_window_count: markerWindows.length,
      outbound_candidate_count: candidateCount,
      gap_affected_window_count: gapAffectedWindowCount,
      protocol_proven_candidate_count: 0,
    },
  };
}

function validateEvidence(actions, session, journalSession, hashes, journalRecords) {
  if (actions.schema_version !== 2 || actions.status !== "complete") throw new Error("Action ledger must be complete schema version 2 evidence");
  if (session.schema_version !== 2 || session.status !== "complete" || session.capture_purpose !== "marker-audit") throw new Error("Session must be a complete schema version 2 marker-audit capture");
  if (!journalSession || journalSession.format_version !== 1) throw new Error("Protocol journal session is missing or unsupported");
  for (const [label, id] of [["action ledger", actions.capture_id], ["journal", journalSession.capture_id]]) {
    if (id !== session.capture_id) throw new Error(`${label} capture_id does not match session manifest`);
  }
  if (actions.scene?.id !== session.scene?.id || actions.scene?.name !== session.scene?.name) throw new Error("Action ledger scene does not match session manifest");
  if (String(actions.game_build) !== String(session.identity?.game_build) || String(actions.game_build) !== String(journalSession.game_build?.build_id)) throw new Error("Game build identity mismatch across marker evidence");
  if (actions.protocol_pack_digest !== session.identity?.protocol_pack_digest || actions.protocol_pack_digest !== journalSession.protocol_pack_digest) throw new Error("Protocol-pack digest mismatch across marker evidence");
  validateProtocolAuthority(session.identity?.protocol_pack_authority, journalSession.protocol_pack_authority, actions.game_build);
  if (actions.action_plan_sha256 !== session.action_plan?.sha256) throw new Error("Action-plan hash mismatch across marker evidence");
  const actionEvidence = session.evidence?.marker_actions;
  if (!actionEvidence || actionEvidence.sha256 !== hashes.actions || actionEvidence.count !== actions.actions?.length) throw new Error("Session manifest does not bind the supplied action ledger hash/count");
  const journalArtifact = session.artifacts?.find((entry) => entry.kind === "protocol_journal");
  if (journalArtifact && journalArtifact.sha256 !== hashes.journal) throw new Error("Session manifest protocol-journal hash mismatch");
  if (!Array.isArray(actions.actions) || actions.actions.length < 1 || actions.actions.length > 6) throw new Error("Action ledger must contain 1 through 6 marker actions");

  const sessionStart = isoMicros(session.session_started_utc, "session_started_utc");
  const captureReady = isoMicros(session.capture_ready_utc, "capture_ready_utc");
  const sessionEnd = isoMicros(session.session_ended_utc, "session_ended_utc");
  if (!(sessionStart <= captureReady && captureReady < sessionEnd)) throw new Error("Session timestamps are disordered");
  let priorResult = captureReady;
  actions.actions.forEach((action, index) => {
    if (action.marker_number !== index + 1) throw new Error(`Marker action ${index + 1} is out of sequence`);
    for (const field of ["placement_ready_unix_micros", "placement_completed_unix_micros", "result_recorded_unix_micros"]) {
      if (!Number.isSafeInteger(action[field])) throw new Error(`Marker ${index + 1} ${field} must be a safe integer`);
    }
    const { placement_ready_unix_micros: ready, placement_completed_unix_micros: completed, result_recorded_unix_micros: result } = action;
    if (!(priorResult <= ready && ready < completed && completed <= result && result <= sessionEnd)) throw new Error(`Marker ${index + 1} timestamps are disordered or outside the capture session`);
    priorResult = result;
  });
  if (journalSession.started_unix_micros > actions.actions[0].placement_ready_unix_micros) throw new Error("Protocol journal starts after the first marker window");
  const lastJournalTime = journalRecords.at(-1)?.wall_clock_unix_micros;
  if (!Number.isSafeInteger(lastJournalTime) || lastJournalTime < actions.actions.at(-1).result_recorded_unix_micros) throw new Error("Protocol journal ends before the final marker result timestamp");
}

function validateProtocolAuthority(sessionAuthority, journalAuthority, capturedBuild) {
  const rawKind = "unverified-carry-forward-decoder-hypothesis";
  if (sessionAuthority?.kind !== rawKind) {
    if (journalAuthority != null) throw new Error("Protocol-pack authority mismatch across marker evidence");
    return;
  }
  if (journalAuthority?.kind !== rawKind) throw new Error("Raw capture journal is missing its unverified carry-forward authority");
  for (const field of ["kind", "source_build", "captured_build", "exact_for_captured_build", "runtime_authority"]) {
    if (sessionAuthority[field] !== journalAuthority[field]) throw new Error(`Protocol-pack authority ${field} mismatch across marker evidence`);
  }
  if (String(sessionAuthority.captured_build) !== String(capturedBuild) ||
      sessionAuthority.source_build === sessionAuthority.captured_build ||
      sessionAuthority.exact_for_captured_build !== false ||
      sessionAuthority.runtime_authority !== false) {
    throw new Error("Raw capture protocol-pack authority is invalid");
  }
}

function readJournal(file) {
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
  let session = null;
  const records = [];
  let priorSequence = 0;
  for (let index = 0; index < lines.length; index += 1) {
    if (!lines[index].trim()) continue;
    let row;
    try { row = JSON.parse(lines[index]); } catch (error) { throw new Error(`Invalid protocol journal JSON at line ${index + 1}: ${error.message}`); }
    if (row.line === "session") {
      if (session || records.length) throw new Error("Protocol journal session must be the first and only session row");
      session = row.data;
    } else if (row.line === "record") {
      if (!session) throw new Error("Protocol journal record precedes session row");
      if (!Number.isSafeInteger(row.data?.sequence) || row.data.sequence !== priorSequence + 1) throw new Error(`Protocol journal sequence discontinuity at line ${index + 1}`);
      if (!Number.isSafeInteger(row.data.wall_clock_unix_micros)) throw new Error(`Protocol journal timestamp is invalid at line ${index + 1}`);
      if (records.length && row.data.wall_clock_unix_micros < records.at(-1).wall_clock_unix_micros) throw new Error(`Protocol journal timestamps regress at line ${index + 1}`);
      priorSequence = row.data.sequence;
      records.push(row.data);
      if (records.length > MAX_RECORDS) throw new Error(`Protocol journal exceeds ${MAX_RECORDS} records`);
    } else throw new Error(`Unsupported protocol journal row at line ${index + 1}`);
  }
  if (!session) throw new Error("Protocol journal session row was not observed");
  return { session, records };
}

function normalizePackets(records) {
  const rawPackets = records.flatMap((record) => record.kind?.record === "packet" ? [normalizePacket(record)] : []);
  const childCoordinates = new Set(rawPackets.filter((packet) => !["frame_up", "frame_down"].includes(packet.fragment_kind)).map(packetCoordinate));
  const seen = new Set();
  return rawPackets.filter((packet) => {
    if (["frame_up", "frame_down"].includes(packet.fragment_kind) && childCoordinates.has(packetCoordinate(packet))) return false;
    const key = [packetCoordinate(packet), packet.fragment_kind, packet.route_signature, packet.payload_sha256].join("|");
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function normalizePacket(record) {
  const packet = record.kind.data;
  if (!Number.isSafeInteger(packet.connection_id) || !Number.isSafeInteger(packet.stream_id)) {
    throw new Error(`Protocol packet ${record.sequence} has an invalid connection or stream identity`);
  }
  if (!['client_to_server', 'server_to_client'].includes(packet.direction)) {
    throw new Error(`Protocol packet ${record.sequence} has an invalid direction`);
  }
  if (typeof packet.fragment?.kind !== "string" || packet.fragment.kind.length === 0) {
    throw new Error(`Protocol packet ${record.sequence} has no fragment kind`);
  }
  const applicationBytes = packet.payload?.application_bytes ?? [];
  if (!Array.isArray(applicationBytes) || applicationBytes.some((value) => !Number.isInteger(value) || value < 0 || value > 255)) {
    throw new Error(`Protocol packet ${record.sequence} has invalid application bytes`);
  }
  const key = packet.route?.key ?? {};
  const bytes = Buffer.from(applicationBytes);
  const routeSignature = key.service_id == null
    ? `${packet.direction}:${packet.fragment?.kind ?? "unknown"}`
    : `${packet.direction}:${packet.fragment?.kind ?? "unknown"}:${key.service_id}:${key.method_id}`;
  return {
    sequence: record.sequence,
    wall_clock_unix_micros: record.wall_clock_unix_micros,
    connection_id: packet.connection_id,
    stream_id: packet.stream_id,
    direction: packet.direction,
    fragment_kind: packet.fragment?.kind ?? "unknown",
    wire_id: packet.fragment?.wire_id ?? null,
    service_id: key.service_id ?? null,
    method_id: key.method_id ?? null,
    stub_id: packet.route?.stub_id ?? null,
    call_id: packet.route?.call_id ?? null,
    route_signature: routeSignature,
    application_bytes: bytes.length,
    payload_sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function buildWindow(action, index, allActions, flowWindows, packets, gaps, baselineMicros, responseMicros) {
  const start = action.placement_ready_unix_micros;
  const completed = action.placement_completed_unix_micros;
  const flowWindow = flowWindows.find((entry) => entry.marker_number === action.marker_number);
  const declaredEndSeconds = flowWindow?.response_window_end_epoch_seconds;
  if (declaredEndSeconds != null && (!Number.isFinite(declaredEndSeconds) || declaredEndSeconds * 1_000_000 < completed)) {
    throw new Error(`Marker ${action.marker_number} action-flow response window is invalid`);
  }
  const end = declaredEndSeconds == null ? completed + responseMicros : Math.round(declaredEndSeconds * 1_000_000);
  const priorBoundary = index === 0 ? -Infinity : allActions[index - 1].result_recorded_unix_micros;
  const baselineStart = Math.max(start - baselineMicros, priorBoundary);
  return {
    marker_number: action.marker_number,
    expected_action: action.expected_action,
    start,
    completed,
    end,
    baselineStart,
    baseline: packets.filter((packet) => packet.direction === "client_to_server" && packet.wall_clock_unix_micros >= baselineStart && packet.wall_clock_unix_micros < start),
    baselineInbound: packets.filter((packet) => packet.direction === "server_to_client" && packet.wall_clock_unix_micros >= baselineStart && packet.wall_clock_unix_micros < start),
    outbound: packets.filter((packet) => packet.direction === "client_to_server" && packet.wall_clock_unix_micros >= start && packet.wall_clock_unix_micros <= completed),
    inbound: packets.filter((packet) => packet.direction === "server_to_client" && packet.wall_clock_unix_micros >= start && packet.wall_clock_unix_micros <= end),
    gaps: gaps.filter((gap) => gap.wall_clock_unix_micros >= baselineStart && gap.wall_clock_unix_micros <= end),
  };
}

function normalizeGaps(records) {
  return records.flatMap((record) => {
    if (record.kind?.record !== "gap") return [];
    const gap = record.kind.data ?? {};
    return [{
      sequence: record.sequence,
      wall_clock_unix_micros: record.wall_clock_unix_micros,
      gap_kind: gap.kind ?? "unknown",
      connection_id: gap.connection_id ?? null,
      stream_id: gap.stream_id ?? null,
      lost_bytes: gap.lost_bytes ?? null,
      detail: gap.detail ?? null,
    }];
  });
}

function publicPacket(packet) {
  return {
    sequence: packet.sequence,
    wall_clock_unix_micros: packet.wall_clock_unix_micros,
    connection_id: packet.connection_id,
    stream_id: packet.stream_id,
    direction: packet.direction,
    fragment_kind: packet.fragment_kind,
    wire_id: packet.wire_id,
    service_id: packet.service_id,
    method_id: packet.method_id,
    stub_id: packet.stub_id,
    call_id: packet.call_id,
    route_signature: packet.route_signature,
    application_bytes: packet.application_bytes,
    payload_sha256: packet.payload_sha256,
  };
}

function returnCorrelationBasis(outbound, inbound) {
  if (outbound.call_id != null && inbound.call_id === outbound.call_id) return "matching-call-id";
  if (inbound.fragment_kind === "return") return "same-connection-subsequent-return";
  return "same-connection-subsequent-inbound";
}

function packetCoordinate(packet) { return `${packet.wall_clock_unix_micros}|${packet.direction}|${packet.connection_id}`; }
function counts(items) { const result = Object.create(null); for (const item of items) result[item] = (result[item] ?? 0) + 1; return result; }
function sha256File(file) { return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex"); }
function verifyExpectedHash(actual, expected, label) { if (expected && actual !== expected) throw new Error(`${label} hash does not match the explicitly expected SHA-256`); }
function optionalSha(value, label) { if (value == null) return null; const normalized = String(value).replace(/^sha256:/, ""); if (!/^[0-9a-f]{64}$/.test(normalized)) throw new Error(`${label} must be a lowercase SHA-256`); return normalized; }
function isoMicros(value, label) {
  const millis = Date.parse(value);
  if (!Number.isFinite(millis)) throw new Error(`${label} is not an ISO-8601 timestamp`);
  const fraction = String(value).match(/\.(\d+)/)?.[1] ?? "";
  const microsWithinSecond = Number((fraction + "000000").slice(0, 6));
  return Math.trunc(millis / 1000) * 1_000_000 + microsWithinSecond;
}
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return crypto.createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function readJson(file, label) { try { return JSON.parse(fs.readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to parse ${label}: ${error.message}`); } }
function requireFile(file, label) { if (!fs.existsSync(file) || !fs.statSync(file).isFile()) throw new Error(`${label} does not exist: ${file}`); }
function requiredPath(values, name) { const resolved = path.resolve(required(values, name)); requireFile(resolved, `--${name}`); return resolved; }
function required(values, name) { const value = values[name]; if (!value) throw new Error(`Missing --${name}`); return value; }
function parsePositive(value, fallback, label) { const number = value == null ? fallback : Number(value); if (!Number.isSafeInteger(number) || number <= 0) throw new Error(`${label} must be a positive integer`); return number; }
function refuseExisting(file) { if (fs.existsSync(file) || fs.existsSync(`${file}.partial`)) throw new Error(`Refusing to overwrite output: ${file}`); }
function parseArgs(args) { const values = {}; for (let i = 0; i < args.length; i += 2) { const token = args[i]; if (!token?.startsWith("--") || args[i + 1] == null) usage(1); values[token.slice(2)] = args[i + 1]; } return values; }
function validateReportEnvelope(report) { if (report.schema_version !== SCHEMA_VERSION || report.generated_by !== GENERATED_BY || !report.inputs) throw new Error("Unsupported marker correlation report"); }

function usage(code) {
  process.stderr.write(`Usage:\n  node ${GENERATED_BY} analyze --actions <marker-actions.json> --session <marker-session.json> --journal <protocol.jsonl> --output <report.json> [--actions-sha256 <hash>] [--session-sha256 <hash>] [--journal-sha256 <hash>] [--baseline-ms 3000] [--response-ms 3000]\n  node ${GENERATED_BY} verify --report <report.json>\n  node ${GENERATED_BY} self-test\n`);
  process.exit(code);
}

function selfTest() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rlogs-marker-correlation-"));
  try {
    const actionsPath = path.join(root, "capture.marker-actions.json");
    const sessionPath = path.join(root, "capture.marker-session.json");
    const journalPath = path.join(root, "capture.protocol.jsonl");
    const outputPath = path.join(root, "report.json");
    const base = 1_800_000_000_000_000;
    const action = { marker_number: 1, expected_action: "place marker 1", placement_ready_unix_micros: base + 5_000_000, placement_completed_unix_micros: base + 5_500_000, result_recorded_unix_micros: base + 5_700_000 };
    const actions = { schema_version: 2, status: "complete", capture_id: "fixture", scene: { id: 1, name: "Fixture" }, game_build: "42", protocol_pack_digest: `sha256:${"a".repeat(64)}`, action_plan_sha256: "b".repeat(64), actions: [action] };
    fs.writeFileSync(actionsPath, JSON.stringify(actions));
    const packet = (sequence, time, direction, fragment, bytes, route = null) => ({ line: "record", data: { sequence, observed_micros: time - base, wall_clock_unix_micros: time, kind: { record: "packet", data: { connection_id: 7, stream_id: direction === "client_to_server" ? 1 : 2, direction, fragment: { kind: fragment }, route, payload: { application_bytes: bytes } } } } });
    const journalRows = [
      { line: "session", data: { format_version: 1, capture_id: "fixture", started_unix_micros: base, game_build: { build_id: "42" }, protocol_pack_digest: actions.protocol_pack_digest } },
      packet(1, base + 3_000_000, "client_to_server", "notify", [1], { key: { service_id: 9, method_id: 1 }, stub_id: 0, call_id: null }),
      packet(2, base + 5_100_000, "client_to_server", "frame_up", [0, 2]),
      packet(3, base + 5_100_000, "client_to_server", "notify", [2], { key: { service_id: 9, method_id: 2 }, stub_id: 4, call_id: 12 }),
      packet(4, base + 5_200_000, "server_to_client", "return", [3], { key: { service_id: 9, method_id: 2 }, stub_id: 4, call_id: 12 }),
      { line: "record", data: { sequence: 5, observed_micros: 5_300_000, wall_clock_unix_micros: base + 5_300_000, kind: { record: "gap", data: { kind: "capture_loss", connection_id: 7, stream_id: 2, lost_bytes: 8, detail: "fixture" } } } },
      packet(6, base + 6_000_000, "server_to_client", "notify", [4]),
    ];
    fs.writeFileSync(journalPath, `${journalRows.map(JSON.stringify).join("\n")}\n`);
    const session = { schema_version: 2, status: "complete", capture_id: "fixture", capture_purpose: "marker-audit", session_started_utc: new Date(base / 1000).toISOString(), capture_ready_utc: new Date((base + 1_000_000) / 1000).toISOString(), session_ended_utc: new Date((base + 10_000_000) / 1000).toISOString(), scene: actions.scene, identity: { game_build: "42", protocol_pack_digest: actions.protocol_pack_digest }, action_plan: { sha256: actions.action_plan_sha256 }, evidence: { marker_actions: { sha256: sha256File(actionsPath), count: 1 } }, artifacts: [] };
    fs.writeFileSync(sessionPath, JSON.stringify(session));
    const report = analyze({ actionsPath, sessionPath, journalPath, baselineMicros: 3_000_000, responseMicros: 3_000_000 });
    assert.equal(report.marker_windows.length, 1);
    assert.equal(report.marker_windows[0].candidates.length, 1, "frame wrapper should be deduplicated in favor of decoded child");
    assert.equal(report.marker_windows[0].candidates[0].method_id, 2);
    assert.equal(report.marker_windows[0].candidates[0].correlated_server_return.correlation_basis, "matching-call-id");
    assert.equal(report.marker_windows[0].candidates[0].route_seen_in_baseline, false);
    assert.equal(report.marker_windows[0].integrity, "gap-observed");
    assert.equal(report.marker_windows[0].gaps[0].lost_bytes, 8);
    assert.equal(report.summary.protocol_proven_candidate_count, 0);
    assert.throws(() => analyze({ actionsPath, sessionPath, journalPath, expectedJournalSha256: "0".repeat(64), baselineMicros: 3_000_000, responseMicros: 3_000_000 }), /explicitly expected/);

    const rawAuthority = { kind: "unverified-carry-forward-decoder-hypothesis", source_build: "41", captured_build: "42", exact_for_captured_build: false, runtime_authority: false };
    session.identity.protocol_pack_authority = rawAuthority;
    journalRows[0].data.protocol_pack_authority = rawAuthority;
    fs.writeFileSync(sessionPath, JSON.stringify(session));
    fs.writeFileSync(journalPath, `${journalRows.map(JSON.stringify).join("\n")}\n`);
    assert.equal(analyze({ actionsPath, sessionPath, journalPath, baselineMicros: 3_000_000, responseMicros: 3_000_000 }).capture.game_build, "42");
    journalRows[0].data.protocol_pack_authority = { ...rawAuthority, source_build: "40" };
    fs.writeFileSync(journalPath, `${journalRows.map(JSON.stringify).join("\n")}\n`);
    assert.throws(() => analyze({ actionsPath, sessionPath, journalPath, baselineMicros: 3_000_000, responseMicros: 3_000_000 }), /authority.*mismatch/i);
    journalRows[0].data.protocol_pack_authority = rawAuthority;
    fs.writeFileSync(journalPath, `${journalRows.map(JSON.stringify).join("\n")}\n`);

    report.content_sha256 = contentHash(report);
    fs.writeFileSync(outputPath, JSON.stringify(report));
    validateReportEnvelope(readJson(outputPath, "test report"));
    assert.equal(contentHash(report), report.content_sha256);

    const corrupted = structuredClone(actions);
    corrupted.capture_id = "wrong";
    fs.writeFileSync(actionsPath, JSON.stringify(corrupted));
    assert.throws(() => analyze({ actionsPath, sessionPath, journalPath, baselineMicros: 3_000_000, responseMicros: 3_000_000 }), /capture_id|bind/);
    process.stdout.write("marker action correlation self-test passed\n");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}
