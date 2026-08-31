#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync,
  createReadStream,
  existsSync,
  mkdtempSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const GENERATED_BY = "tools/bpsr-protocol-gap-ledger.mjs";
const SCHEMA_VERSION = 1;
const MAXIMUM_LINE_BYTES = 128 * 1024 * 1024;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") await generateCommand(options);
else if (command === "verify") await verifyCommand(options);
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function generateCommand(values) {
  const build = required(values, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  const auditFile = resolvePath(required(values, "audit"));
  const reportsRoot = resolvePath(required(values, "reports-root"));
  const journalsRoot = resolvePath(required(values, "journals-root"));
  const outputFile = resolvePath(required(values, "output"));
  if (existsSync(outputFile)) throw new Error(`Refusing to overwrite ${outputFile}`);

  const ledger = await buildLedger({ build, auditFile, reportsRoot, journalsRoot });
  mkdirSync(path.dirname(outputFile), { recursive: true });
  writeFileSync(outputFile, `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await verifyLedger(ledger, true);
  console.log(
    `Protocol gap ledger for build ${build}: ${ledger.summary.capture_gap_count} conserved gaps `
      + `across ${ledger.summary.journal_count} bounded journal streams; global conservation proven=false.`,
  );
  console.log(`wrote ${relativePath(outputFile)}`);
}

async function verifyCommand(values) {
  const inputFile = resolvePath(required(values, "input"));
  const ledger = readJson(inputFile, "protocol gap ledger");
  await verifyLedger(ledger, true);
  console.log(
    `Protocol gap ledger verified for build ${ledger.game_build}: `
      + `${ledger.summary.capture_gap_count} gaps, zero hidden reconciliation differences.`,
  );
}

async function buildLedger({ build, auditFile, reportsRoot, journalsRoot }) {
  requireFile(auditFile, "protocol promotion audit");
  requireDirectory(reportsRoot, "offline recording reports root");
  requireDirectory(journalsRoot, "protocol journals root");
  const audit = readJson(auditFile, "protocol promotion audit");
  if (String(audit.build_id ?? "") !== build) {
    throw new Error(`Audit build ${audit.build_id ?? "<missing>"} does not match ${build}`);
  }
  if (!Array.isArray(audit.report_paths) || audit.report_paths.length === 0) {
    throw new Error("Protocol promotion audit has no recording reports");
  }

  const sessions = [];
  for (const auditedReportPath of audit.report_paths) {
    const reportName = path.basename(auditedReportPath);
    if (!reportName.endsWith(".protocol.offline-recording-report.json")) {
      throw new Error(`Unexpected recording report name ${reportName}`);
    }
    const reportFile = path.join(reportsRoot, reportName);
    const journalName = reportName.replace(/\.offline-recording-report\.json$/, ".jsonl");
    const journalFile = path.join(journalsRoot, journalName);
    requireFile(reportFile, "offline recording report");
    requireFile(journalFile, "protocol journal");
    const report = readJson(reportFile, `offline recording report ${reportName}`);
    validateReportIdentity(report, audit, reportName);
    const journal = await analyzeJournal(journalFile, build, report.protocol_pack_transition);
    const reportGapCounts = Object.fromEntries(
      (report.gaps ?? []).map((entry) => [String(entry.kind), Number(entry.count)]),
    );
    const journalGapCounts = countBy(journal.gaps, (gap) => gap.kind);
    requireEqualCounts(reportGapCounts, journalGapCounts, `${reportName} gap kinds`);
    if (Number(report.record_count) !== journal.replayed_record_count ||
      Number(report.capture?.packet_count) !== journal.packet_count ||
      Number(report.capture?.gap_count) !== journal.gaps.length) {
      throw new Error(`Journal/report record conservation failed for ${reportName}`);
    }

    sessions.push({
      capture_id: journal.capture_id,
      report: await fileReceipt(reportFile),
      journal: journal.receipt,
      maximum_line_bytes: journal.maximum_line_bytes,
      valid_journal_record_count: journal.valid_record_count,
      replayed_record_count: journal.replayed_record_count,
      packet_count: journal.packet_count,
      capture_gap_count: journal.gaps.length,
      recovered_truncated_tail_count: journal.recovered_truncated_tail_count,
      gap_counts: sortedCounts(journalGapCounts),
      report_record_conservation_proven: true,
      report_gap_conservation_proven: true,
      gaps: journal.gaps,
    });
  }

  const allGaps = sessions.flatMap((session) => session.gaps);
  const kindCounts = countBy(allGaps, (gap) => gap.kind);
  const classificationCounts = summarizeClassifications(allGaps);
  const sourceIdentities = uniqueJson(
    sessions.map((session) => {
      const report = readJson(resolvePath(session.report.path), "transition report");
      const transition = report.protocol_pack_transition;
      return {
        protocol_pack_id: transition.source_protocol_pack_id,
        protocol_pack_digest: transition.source_protocol_pack_digest,
      };
    }),
  );
  if (sourceIdentities.length !== 1) {
    throw new Error(`Expected one journal source-pack identity, found ${sourceIdentities.length}`);
  }
  const captureGapCount = allGaps.length;
  if (captureGapCount !== Number(audit.capture_gap_count)) {
    throw new Error(
      `Audit gap conservation failed: journals ${captureGapCount}, audit ${audit.capture_gap_count}`,
    );
  }

  const ledger = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: build,
    policy: {
      journals_are_streamed_one_line_at_a_time: true,
      maximum_jsonl_line_bytes: MAXIMUM_LINE_BYTES,
      every_replay_gap_is_preserved_exactly_once: true,
      truncated_final_json_is_an_explicit_malformed_frame_gap: true,
      tcp_and_framing_discontinuities_block_global_conservation: true,
      packet_absence_is_not_zero: true,
      remote_player_packet_acquisition_required: false,
      structural_non_obligations_never_synthesize_events: true,
      event_local_windows_must_exclude_every_gap_boundary: true,
    },
    inputs: {
      audit: await fileReceipt(auditFile),
      reports_root: relativePath(reportsRoot),
      journals_root: relativePath(journalsRoot),
      destination_protocol_pack_id: audit.protocol_pack_id,
      destination_protocol_pack_digest: audit.protocol_pack_digest,
      source_protocol_pack_id: sourceIdentities[0].protocol_pack_id,
      source_protocol_pack_digest: sourceIdentities[0].protocol_pack_digest,
    },
    summary: {
      report_count: sessions.length,
      journal_count: sessions.length,
      valid_journal_record_count: sum(sessions, "valid_journal_record_count"),
      replayed_record_count: sum(sessions, "replayed_record_count"),
      packet_count: sum(sessions, "packet_count"),
      capture_gap_count: captureGapCount,
      recovered_truncated_tail_count: sum(sessions, "recovered_truncated_tail_count"),
      reports_with_any_gap: sessions.filter((session) => session.capture_gap_count > 0).length,
      reports_with_tcp_gap: sessions.filter((session) =>
        session.gaps.some((gap) => gap.kind === "tcp_gap")
      ).length,
      gap_kind_counts: sortedCounts(kindCounts),
      gap_classification_counts: classificationCounts,
      maximum_observed_line_bytes: Math.max(...sessions.map((session) => session.maximum_line_bytes)),
      report_record_conservation_proven: true,
      report_gap_conservation_proven: true,
      global_canonical_replay_conservation_proven: captureGapCount === 0,
      event_local_window_isolation_required: captureGapCount > 0,
      remote_player_packet_acquisition_required: false,
    },
    sessions,
    blockers: captureGapCount === 0 ? [] : [
      `${captureGapCount} explicit replay gaps prevent global canonical conservation`,
      `${Number(kindCounts.tcp_gap ?? 0)} TCP-gap records require gap-free matching-build acquisition or strict event-local exclusion`,
      `${Number(kindCounts.malformed_frame ?? 0)} malformed/resynchronization records require classification and strict event-local exclusion`,
    ],
  };
  ledger.content_sha256 = contentDigest(ledger);
  return ledger;
}

function validateReportIdentity(report, audit, reportName) {
  if (Number(report.schema_version) < 4 ||
    report.protocol_pack_id !== audit.protocol_pack_id ||
    report.protocol_pack_digest !== audit.protocol_pack_digest) {
    throw new Error(`Recording report identity mismatch for ${reportName}`);
  }
  const transition = report.protocol_pack_transition;
  if (!transition || transition.policy !== "monotonic_allowed_to_opaque_only" ||
    transition.destination_protocol_pack_id !== report.protocol_pack_id ||
    transition.destination_protocol_pack_digest !== report.protocol_pack_digest ||
    !Number.isSafeInteger(Number(transition.demoted_route_count)) ||
    Number(transition.demoted_route_count) < 0 ||
    !String(transition.source_protocol_pack_id ?? "") ||
    !String(transition.source_protocol_pack_digest ?? "")) {
    throw new Error(`Recording report lacks safe transition provenance for ${reportName}`);
  }
}

async function analyzeJournal(journalFile, build, transition) {
  const terminatedWithNewline = fileEndsWithNewline(journalFile);
  const digest = createHash("sha256");
  const input = createReadStream(journalFile, { highWaterMark: 64 * 1024 });
  input.on("data", (chunk) => digest.update(chunk));
  const lines = createInterface({ input, crlfDelay: Infinity });
  let lineNumber = 0;
  let maximumLineBytes = 0;
  let session = null;
  let validRecordCount = 0;
  let packetCount = 0;
  let previousObservedMicros = 0;
  let invalidTail = null;
  const gaps = [];

  for await (const line of lines) {
    lineNumber += 1;
    const lineBytes = Buffer.byteLength(line, "utf8");
    maximumLineBytes = Math.max(maximumLineBytes, lineBytes);
    if (lineBytes > MAXIMUM_LINE_BYTES) {
      throw new Error(`Journal line ${lineNumber} exceeds ${MAXIMUM_LINE_BYTES} bytes`);
    }
    if (invalidTail) throw new Error(`Invalid JSON occurs before the final line in ${journalFile}`);
    if (!line.trim()) continue;
    let parsed;
    try {
      parsed = JSON.parse(line);
    } catch (error) {
      invalidTail = { line_number: lineNumber, bytes: lineBytes, error: String(error.message ?? error) };
      continue;
    }
    if (!session) {
      if (parsed.line !== "session") throw new Error(`Journal record precedes session in ${journalFile}`);
      session = parsed.data;
      if (String(session.game_build?.build_id ?? "") !== build ||
        session.protocol_pack_digest !== transition.source_protocol_pack_digest) {
        throw new Error(`Journal session identity mismatch in ${journalFile}`);
      }
      continue;
    }
    if (parsed.line !== "record") throw new Error(`Unexpected journal line ${lineNumber}`);
    const record = parsed.data;
    validRecordCount += 1;
    if (Number(record.sequence) !== validRecordCount) {
      throw new Error(`Journal sequence mismatch at line ${lineNumber}`);
    }
    const observedMicros = Number(record.observed_micros);
    if (!Number.isSafeInteger(observedMicros) || observedMicros < previousObservedMicros) {
      throw new Error(`Journal time mismatch at line ${lineNumber}`);
    }
    previousObservedMicros = observedMicros;
    const recordKind = record.kind?.record;
    if (recordKind === "packet") packetCount += 1;
    else if (recordKind === "gap") {
      gaps.push(classifyGap(record, lineNumber));
    } else {
      throw new Error(`Unknown journal record kind at line ${lineNumber}`);
    }
  }
  if (!session) throw new Error(`Journal lacks session header: ${journalFile}`);
  if (invalidTail && terminatedWithNewline) {
    throw new Error(`Complete malformed final line cannot be recovered in ${journalFile}`);
  }
  if (invalidTail) {
    gaps.push({
      sequence: validRecordCount + 1,
      observed_micros: previousObservedMicros,
      line_number: invalidTail.line_number,
      kind: "malformed_frame",
      classification: "truncated_final_json",
      connection_id: null,
      stream_id: null,
      lost_bytes: invalidTail.bytes,
      stream_offset_delta_bytes: null,
      detail: "protocol journal ended during an unterminated JSONL record; valid prefix retained",
    });
  }
  const bytes = statSync(journalFile).size;
  return {
    capture_id: session.capture_id,
    receipt: {
      path: relativePath(journalFile),
      bytes,
      sha256: digest.digest("hex"),
    },
    maximum_line_bytes: maximumLineBytes,
    valid_record_count: validRecordCount,
    replayed_record_count: validRecordCount + Number(Boolean(invalidTail)),
    packet_count: packetCount,
    recovered_truncated_tail_count: Number(Boolean(invalidTail)),
    gaps,
  };
}

function classifyGap(record, lineNumber) {
  const gap = record.kind.data;
  const kind = String(gap.kind);
  const detail = String(gap.detail ?? "");
  let classification = kind;
  if (kind === "malformed_frame") {
    if (detail.includes("Resynchronized")) classification = "framer_resynchronized";
    else if (detail.includes("StreamDiscontinuity")) classification = "stream_discontinuity";
    else if (detail.includes("IdleTimeout")) classification = "idle_timeout_buffer_flush";
    else classification = "malformed_frame_other";
  }
  const offsets = /expected_offset: (\d+), actual_offset: (\d+)/.exec(detail);
  const offsetDelta = offsets ? Number(offsets[2]) - Number(offsets[1]) : null;
  return {
    sequence: Number(record.sequence),
    observed_micros: Number(record.observed_micros),
    line_number: lineNumber,
    kind,
    classification,
    connection_id: gap.connection_id ?? null,
    stream_id: gap.stream_id ?? null,
    lost_bytes: gap.lost_bytes ?? null,
    stream_offset_delta_bytes: offsetDelta,
    detail,
  };
}

function summarizeClassifications(gaps) {
  const groups = new Map();
  for (const gap of gaps) {
    const group = groups.get(gap.classification) ?? {
      classification: gap.classification,
      count: 0,
      declared_lost_bytes: 0,
      stream_offset_delta_bytes: 0,
    };
    group.count += 1;
    group.declared_lost_bytes += Number(gap.lost_bytes ?? 0);
    group.stream_offset_delta_bytes += Number(gap.stream_offset_delta_bytes ?? 0);
    groups.set(gap.classification, group);
  }
  return [...groups.values()].sort((left, right) =>
    left.classification.localeCompare(right.classification)
  );
}

async function verifyLedger(ledger, verifyFiles) {
  if (Number(ledger.schema_version) !== SCHEMA_VERSION || ledger.generated_by !== GENERATED_BY) {
    throw new Error("Unsupported protocol gap ledger schema or generator");
  }
  if (!/^\d+$/.test(String(ledger.game_build ?? "")) ||
    ledger.policy?.journals_are_streamed_one_line_at_a_time !== true ||
    ledger.policy?.every_replay_gap_is_preserved_exactly_once !== true ||
    ledger.policy?.packet_absence_is_not_zero !== true ||
    ledger.policy?.remote_player_packet_acquisition_required !== false ||
    ledger.summary?.remote_player_packet_acquisition_required !== false ||
    ledger.summary?.report_record_conservation_proven !== true ||
    ledger.summary?.report_gap_conservation_proven !== true ||
    !Array.isArray(ledger.sessions) || ledger.sessions.length === 0) {
    throw new Error("Protocol gap ledger has unsafe policy or summary accounting");
  }
  if (ledger.content_sha256 !== contentDigest(ledger)) {
    throw new Error("Protocol gap ledger content digest mismatch");
  }
  const gaps = ledger.sessions.flatMap((session) => session.gaps ?? []);
  if (gaps.length !== Number(ledger.summary.capture_gap_count) ||
    sum(ledger.sessions, "replayed_record_count") !== Number(ledger.summary.replayed_record_count) ||
    sum(ledger.sessions, "packet_count") !== Number(ledger.summary.packet_count) ||
    ledger.sessions.some((session) =>
      session.report_record_conservation_proven !== true ||
      session.report_gap_conservation_proven !== true ||
      Number(session.capture_gap_count) !== session.gaps.length
    )) {
    throw new Error("Protocol gap ledger session totals do not conserve");
  }
  const kindCounts = Object.fromEntries(
    (ledger.summary.gap_kind_counts ?? []).map((entry) => [entry.key, Number(entry.count)]),
  );
  requireEqualCounts(kindCounts, countBy(gaps, (gap) => gap.kind), "ledger gap kinds");
  if ((gaps.length === 0) !== Boolean(ledger.summary.global_canonical_replay_conservation_proven) ||
    (gaps.length > 0) !== Boolean(ledger.summary.event_local_window_isolation_required) ||
    (gaps.length > 0) !== (ledger.blockers.length > 0)) {
    throw new Error("Protocol gap ledger promotion state is inconsistent");
  }
  if (verifyFiles) {
    await verifyReceipt(ledger.inputs.audit, "protocol promotion audit");
    for (const session of ledger.sessions) {
      await verifyReceipt(session.report, "offline recording report");
      await verifyReceipt(session.journal, "protocol journal");
    }
  }
}

async function fileReceipt(file) {
  return {
    path: relativePath(file),
    bytes: statSync(file).size,
    sha256: await hashFile(file),
  };
}

async function verifyReceipt(receipt, label) {
  const file = resolvePath(receipt.path);
  requireFile(file, label);
  if (statSync(file).size !== Number(receipt.bytes) || await hashFile(file) !== receipt.sha256) {
    throw new Error(`${label} receipt mismatch: ${receipt.path}`);
  }
}

async function hashFile(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file, { highWaterMark: 1024 * 1024 })) hash.update(chunk);
  return hash.digest("hex");
}

function contentDigest(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return `sha256:${createHash("sha256").update(JSON.stringify(copy)).digest("hex")}`;
}

function fileEndsWithNewline(file) {
  const size = statSync(file).size;
  if (size === 0) return false;
  const descriptor = openSync(file, "r");
  try {
    const byte = Buffer.allocUnsafe(1);
    readSync(descriptor, byte, 0, 1, size - 1);
    return byte[0] === 0x0a;
  } finally {
    closeSync(descriptor);
  }
}

function countBy(values, selector) {
  const counts = {};
  for (const value of values) {
    const key = String(selector(value));
    counts[key] = Number(counts[key] ?? 0) + 1;
  }
  return counts;
}

function sortedCounts(counts) {
  return Object.entries(counts)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, count]) => ({ key, count: Number(count) }));
}

function requireEqualCounts(expected, actual, label) {
  const keys = [...new Set([...Object.keys(expected), ...Object.keys(actual)])].sort();
  if (keys.some((key) => Number(expected[key] ?? 0) !== Number(actual[key] ?? 0))) {
    throw new Error(`${label} do not conserve: expected ${JSON.stringify(expected)}, actual ${JSON.stringify(actual)}`);
  }
}

function sum(values, field) {
  return values.reduce((total, value) => total + Number(value[field] ?? 0), 0);
}

function uniqueJson(values) {
  return [...new Map(values.map((value) => [JSON.stringify(value), value])).values()];
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`Unable to read ${label} ${file}: ${error.message}`, { cause: error });
  }
}

function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}

function requireDirectory(directory, label) {
  if (!existsSync(directory) || !statSync(directory).isDirectory()) {
    throw new Error(`Missing ${label}: ${directory}`);
  }
}

function resolvePath(value) {
  return path.isAbsolute(value) ? path.normalize(value) : path.resolve(repoRoot, value);
}

function relativePath(value) {
  return path.relative(repoRoot, value).replaceAll("\\", "/");
}

function parseArgs(args) {
  const values = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined) usage(1);
    values[flag.slice(2)] = value;
  }
  return values;
}

function required(values, key) {
  if (!values[key]) throw new Error(`Missing --${key}`);
  return values[key];
}

async function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-protocol-gap-ledger-"));
  try {
    const reportsRoot = path.join(root, "reports");
    const journalsRoot = path.join(root, "journals");
    mkdirSync(reportsRoot);
    mkdirSync(journalsRoot);
    const reportName = "one.protocol.offline-recording-report.json";
    const reportFile = path.join(reportsRoot, reportName);
    const journalFile = path.join(journalsRoot, "one.protocol.jsonl");
    const auditFile = path.join(root, "audit.json");
    const transition = {
      policy: "monotonic_allowed_to_opaque_only",
      source_protocol_pack_id: "source-v2",
      source_protocol_pack_digest: "sha256:source",
      destination_protocol_pack_id: "destination-v3",
      destination_protocol_pack_digest: "sha256:destination",
      demoted_route_count: 1,
    };
    writeFileSync(journalFile, [
      JSON.stringify({ line: "session", data: {
        capture_id: "one",
        game_build: { build_id: "123" },
        protocol_pack_digest: "sha256:source",
      } }),
      JSON.stringify({ line: "record", data: {
        sequence: 1, observed_micros: 10, kind: { record: "packet", data: {} },
      } }),
      JSON.stringify({ line: "record", data: {
        sequence: 2,
        observed_micros: 20,
        kind: { record: "gap", data: {
          kind: "tcp_gap", connection_id: null, stream_id: null, lost_bytes: 0,
          detail: "BPSR framing issue: TcpGap",
        } },
      } }),
      "{\"line\":\"record\"",
    ].join("\n"), "utf8");
    writeFileSync(reportFile, `${JSON.stringify({
      schema_version: 4,
      protocol_pack_id: "destination-v3",
      protocol_pack_digest: "sha256:destination",
      protocol_pack_transition: transition,
      record_count: 3,
      capture: { packet_count: 1, gap_count: 2 },
      gaps: [{ kind: "malformed_frame", count: 1 }, { kind: "tcp_gap", count: 1 }],
    })}\n`, "utf8");
    writeFileSync(auditFile, `${JSON.stringify({
      build_id: "123",
      protocol_pack_id: "destination-v3",
      protocol_pack_digest: "sha256:destination",
      report_paths: [reportFile],
      capture_gap_count: 2,
    })}\n`, "utf8");

    const ledger = await buildLedger({
      build: "123", auditFile, reportsRoot, journalsRoot,
    });
    await verifyLedger(ledger, true);
    if (ledger.summary.capture_gap_count !== 2 ||
      ledger.summary.recovered_truncated_tail_count !== 1 ||
      ledger.summary.global_canonical_replay_conservation_proven !== false) {
      throw new Error("Protocol gap ledger self-test accounting failed");
    }
    console.log("bpsr-protocol-gap-ledger self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-protocol-gap-ledger.mjs generate --build <id> --audit <json> --reports-root <dir> --journals-root <dir> --output <json>\n  node tools/bpsr-protocol-gap-ledger.mjs verify --input <json>\n  node tools/bpsr-protocol-gap-ledger.mjs self-test");
  process.exit(exitCode);
}
