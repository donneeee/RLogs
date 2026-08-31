#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
const GENERATOR = "tools/bpsr-protocol-journal-sealed-prefix.mjs";

if (command === "seal") await seal(options);
else if (command === "segment") await segment(options);
else if (command === "verify") verifyReceipt(readJson(required(options, "receipt")));
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function seal(parsed) {
  const build = required(parsed, "build");
  const input = path.resolve(required(parsed, "input"));
  const output = path.resolve(required(parsed, "output"));
  const receiptPath = path.resolve(required(parsed, "receipt"));
  const maxLineBytes = Number(parsed["max-line-bytes"] ?? 128 * 1024 * 1024);
  if (!/^\d+$/.test(build)) throw new Error("--build must contain only ASCII digits");
  if (!Number.isSafeInteger(maxLineBytes) || maxLineBytes <= 0) {
    throw new Error("--max-line-bytes must be a positive safe integer");
  }
  for (const target of [output, receiptPath, `${output}.partial`]) {
    if (fs.existsSync(target)) throw new Error(`refusing to overwrite ${target}`);
  }

  const scan = await scanJournal(input, build, maxLineBytes);
  const outputSession = structuredClone(scan.session);
  outputSession.capture_id = `${scan.session.capture_id}.sealed-prefix-${scan.lastSequence}`;
  outputSession.adapter = {
    name: "audited-complete-prefix",
    version: "1",
  };

  fs.mkdirSync(path.dirname(output), { recursive: true });
  const partial = `${output}.partial`;
  await writePrefix(input, partial, outputSession, scan, maxLineBytes);
  fs.renameSync(partial, output);

  const receipt = {
    schema_version: 1,
    generated_by: GENERATOR,
    game_build: build,
    policy: {
      source_journal_is_modified: false,
      source_complete_records_are_byte_preserved: true,
      output_records_are_resequenced: false,
      explicit_capture_gaps_allowed: false,
      invalid_complete_json_lines_allowed: false,
      only_unterminated_invalid_final_line_may_be_excluded: true,
      excluded_tail_is_treated_as_zero_bytes_or_events: false,
      output_proves_events_after_the_sealed_prefix: false,
      output_proves_encounter_or_lifecycle_conservation: false,
      output_may_prove_decoder_behavior_for_retained_exact_packets: true,
      current_character_snapshots_substituted: false,
    },
    source: descriptor(input),
    output: descriptor(output),
    source_capture_id: scan.session.capture_id,
    output_capture_id: outputSession.capture_id,
    protocol_pack_digest: scan.session.protocol_pack_digest ?? null,
    complete_record_count: scan.recordCount,
    first_source_sequence: scan.recordCount === 0 ? null : 1,
    last_source_sequence: scan.recordCount === 0 ? null : scan.lastSequence,
    explicit_capture_gap_records: 0,
    excluded_unterminated_tail: scan.excludedTail === null ? null : {
      line_number: scan.excludedTail.lineNumber,
      bytes: Buffer.byteLength(scan.excludedTail.text),
      sha256: sha256(Buffer.from(scan.excludedTail.text)),
      parsed_as_event: false,
      treated_as_zero: false,
    },
    authority: {
      gap_free_complete_prefix_proven: true,
      protocol_route_or_decoder_semantics_proven: false,
      canonical_replay_conservation_proven: false,
      runtime_promotion_allowed: false,
      provider_rdps_credit_allowed: false,
    },
  };
  receipt.content_sha256 = contentHash(receipt);
  fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
  fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, "utf8");
  verifyReceipt(readJson(receiptPath));
  console.log(`sealed ${scan.recordCount} complete records from ${input}`);
}

async function segment(parsed) {
  const build = required(parsed, "build");
  const input = path.resolve(required(parsed, "input"));
  const output = path.resolve(required(parsed, "output"));
  const receiptPath = path.resolve(required(parsed, "receipt"));
  const fromSequence = Number(required(parsed, "from-sequence"));
  const toSequence = Number(required(parsed, "to-sequence"));
  const maxLineBytes = Number(parsed["max-line-bytes"] ?? 128 * 1024 * 1024);
  if (!/^\d+$/.test(build)) throw new Error("--build must contain only ASCII digits");
  if (!Number.isSafeInteger(fromSequence) || !Number.isSafeInteger(toSequence) ||
    fromSequence <= 0 || toSequence < fromSequence) {
    throw new Error("segment sequence bounds must be positive and ordered");
  }
  if (!Number.isSafeInteger(maxLineBytes) || maxLineBytes <= 0) {
    throw new Error("--max-line-bytes must be a positive safe integer");
  }
  for (const target of [output, receiptPath, `${output}.partial`]) {
    if (fs.existsSync(target)) throw new Error(`refusing to overwrite ${target}`);
  }

  const scan = await scanSegment(
    input,
    build,
    fromSequence,
    toSequence,
    maxLineBytes,
  );
  const outputSession = structuredClone(scan.session);
  outputSession.capture_id =
    `${scan.session.capture_id}.gap-free-segment-${fromSequence}-${toSequence}`;
  outputSession.adapter = { name: "audited-gap-free-segment", version: "1" };
  fs.mkdirSync(path.dirname(output), { recursive: true });
  const partial = `${output}.partial`;
  await writeSegment(input, partial, outputSession, scan, maxLineBytes);
  fs.renameSync(partial, output);

  const receipt = {
    schema_version: 2,
    artifact_kind: "gap-free-journal-segment",
    generated_by: GENERATOR,
    game_build: build,
    policy: {
      source_journal_is_modified: false,
      source_packet_payload_byte_arrays_are_preserved: true,
      output_record_wrappers_are_resequenced: true,
      selected_capture_gap_records_allowed: false,
      source_capture_gaps_outside_segment_are_disclosed: true,
      gaps_outside_segment_are_treated_as_zero_bytes_or_events: false,
      invalid_complete_json_lines_allowed: false,
      only_unterminated_invalid_final_line_may_be_excluded_from_source_scan: true,
      output_proves_events_before_or_after_the_segment: false,
      output_proves_encounter_or_lifecycle_conservation: false,
      output_may_prove_decoder_behavior_for_retained_exact_packets: true,
      current_character_snapshots_substituted: false,
    },
    source: descriptor(input),
    output: descriptor(output),
    source_capture_id: scan.session.capture_id,
    output_capture_id: outputSession.capture_id,
    protocol_pack_digest: scan.session.protocol_pack_digest ?? null,
    source_record_count: scan.sourceRecordCount,
    source_sequence_start: fromSequence,
    source_sequence_end: toSequence,
    selected_record_count: scan.selectedRecordCount,
    selected_capture_gap_records: 0,
    source_capture_gap_records: scan.sourceGapCount,
    source_records_before_segment: fromSequence - 1,
    source_records_after_segment: scan.sourceRecordCount - toSequence,
    excluded_unterminated_tail: scan.excludedTail === null ? null : {
      line_number: scan.excludedTail.lineNumber,
      bytes: Buffer.byteLength(scan.excludedTail.text),
      sha256: sha256(Buffer.from(scan.excludedTail.text)),
      parsed_as_event: false,
      treated_as_zero: false,
    },
    authority: {
      gap_free_selected_segment_proven: true,
      protocol_route_or_decoder_semantics_proven: false,
      canonical_replay_conservation_proven: false,
      runtime_promotion_allowed: false,
      provider_rdps_credit_allowed: false,
    },
  };
  receipt.content_sha256 = contentHash(receipt);
  fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
  fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, "utf8");
  verifyReceipt(readJson(receiptPath));
  console.log(
    `sealed source records ${fromSequence}-${toSequence} as ${scan.selectedRecordCount} gap-free records`,
  );
}

async function scanSegment(input, build, fromSequence, toSequence, maxLineBytes) {
  if (!fs.statSync(input).isFile()) throw new Error(`input is not a file: ${input}`);
  const endsWithNewline = fileEndsWithNewline(input);
  const reader = readline.createInterface({
    input: fs.createReadStream(input, { encoding: "utf8" }),
    crlfDelay: Infinity,
  });
  let session = null;
  let sourceRecordCount = 0;
  let selectedRecordCount = 0;
  let sourceGapCount = 0;
  let previousObserved = null;
  let excludedTail = null;
  let lineNumber = 0;
  for await (const line of reader) {
    lineNumber += 1;
    if (Buffer.byteLength(line) > maxLineBytes) {
      throw new Error(`line ${lineNumber} exceeds --max-line-bytes`);
    }
    if (line.trim() === "") continue;
    if (excludedTail !== null) {
      throw new Error(`invalid JSON occurred before the final line at ${excludedTail.lineNumber}`);
    }
    let parsed;
    try { parsed = JSON.parse(line); }
    catch (error) {
      excludedTail = { lineNumber, text: line, error: String(error) };
      continue;
    }
    if (session === null) {
      if (parsed?.line !== "session" || typeof parsed?.data !== "object") {
        throw new Error("first nonblank line is not a journal session");
      }
      session = parsed.data;
      if (String(session?.game_build?.build_id) !== build) {
        throw new Error(`journal build ${session?.game_build?.build_id} does not match ${build}`);
      }
      continue;
    }
    if (parsed?.line !== "record" || typeof parsed?.data !== "object") {
      throw new Error(`line ${lineNumber} is not a journal record`);
    }
    const record = parsed.data;
    const expected = sourceRecordCount + 1;
    if (Number(record.sequence) !== expected) {
      throw new Error(`record sequence ${record.sequence} does not match ${expected}`);
    }
    if (previousObserved !== null && Number(record.observed_micros) < previousObserved) {
      throw new Error(`record ${expected} moves observed time backward`);
    }
    const isGap = record?.kind?.record === "gap";
    if (isGap) sourceGapCount += 1;
    if (expected >= fromSequence && expected <= toSequence) {
      if (isGap) throw new Error(`selected segment contains capture gap at record ${expected}`);
      selectedRecordCount += 1;
    }
    sourceRecordCount = expected;
    previousObserved = Number(record.observed_micros);
  }
  if (session === null) throw new Error("journal has no session line");
  if (excludedTail !== null && endsWithNewline) {
    throw new Error(`invalid complete JSON line ${excludedTail.lineNumber} cannot be excluded`);
  }
  if (sourceRecordCount < toSequence) {
    throw new Error(`journal ends at record ${sourceRecordCount}, before ${toSequence}`);
  }
  if (selectedRecordCount !== toSequence - fromSequence + 1) {
    throw new Error("selected record count does not match its exact source interval");
  }
  return { session, sourceRecordCount, selectedRecordCount, sourceGapCount, excludedTail,
    fromSequence, toSequence };
}

async function writeSegment(input, output, outputSession, scan, maxLineBytes) {
  const writer = fs.createWriteStream(output, { encoding: "utf8", flags: "wx" });
  const write = async (value) => {
    if (!writer.write(value)) await new Promise((resolve) => writer.once("drain", resolve));
  };
  await write(`${JSON.stringify({ line: "session", data: outputSession })}\n`);
  const reader = readline.createInterface({
    input: fs.createReadStream(input, { encoding: "utf8" }), crlfDelay: Infinity,
  });
  let outputSequence = 0;
  let lineNumber = 0;
  for await (const line of reader) {
    lineNumber += 1;
    if (Buffer.byteLength(line) > maxLineBytes) throw new Error(`line ${lineNumber} grew`);
    if (lineNumber === scan.excludedTail?.lineNumber) break;
    if (line.trim() === "") continue;
    const parsed = JSON.parse(line);
    if (parsed?.line !== "record") continue;
    const sourceSequence = Number(parsed.data.sequence);
    if (sourceSequence < scan.fromSequence || sourceSequence > scan.toSequence) continue;
    outputSequence += 1;
    parsed.data.sequence = outputSequence;
    await write(`${JSON.stringify(parsed)}\n`);
  }
  await new Promise((resolve, reject) => {
    writer.on("error", reject);
    writer.end(resolve);
  });
  if (outputSequence !== scan.selectedRecordCount) {
    throw new Error(`wrote ${outputSequence} records but validated ${scan.selectedRecordCount}`);
  }
}

async function scanJournal(input, build, maxLineBytes) {
  if (!fs.statSync(input).isFile()) throw new Error(`input is not a file: ${input}`);
  const endsWithNewline = fileEndsWithNewline(input);
  const reader = readline.createInterface({
    input: fs.createReadStream(input, { encoding: "utf8" }),
    crlfDelay: Infinity,
  });
  let session = null;
  let recordCount = 0;
  let lastSequence = 0;
  let previousObserved = null;
  let excludedTail = null;
  let lineNumber = 0;
  for await (const line of reader) {
    lineNumber += 1;
    if (Buffer.byteLength(line) > maxLineBytes) {
      throw new Error(`line ${lineNumber} exceeds --max-line-bytes`);
    }
    if (line.trim() === "") continue;
    if (excludedTail !== null) {
      throw new Error(`invalid JSON occurred before the final line at ${excludedTail.lineNumber}`);
    }
    let parsed;
    try {
      parsed = JSON.parse(line);
    } catch (error) {
      excludedTail = { lineNumber, text: line, error: String(error) };
      continue;
    }
    if (session === null) {
      if (parsed?.line !== "session" || typeof parsed?.data !== "object") {
        throw new Error("first nonblank line is not a journal session");
      }
      session = parsed.data;
      if (String(session?.game_build?.build_id) !== build) {
        throw new Error(`journal build ${session?.game_build?.build_id} does not match ${build}`);
      }
      continue;
    }
    if (parsed?.line !== "record" || typeof parsed?.data !== "object") {
      throw new Error(`line ${lineNumber} is not a journal record`);
    }
    const record = parsed.data;
    const expected = recordCount + 1;
    if (Number(record.sequence) !== expected) {
      throw new Error(`record sequence ${record.sequence} does not match ${expected}`);
    }
    if (previousObserved !== null && Number(record.observed_micros) < previousObserved) {
      throw new Error(`record ${expected} moves observed time backward`);
    }
    if (record?.kind?.record === "gap") {
      throw new Error(`journal contains explicit capture gap at record ${expected}`);
    }
    recordCount = expected;
    lastSequence = Number(record.sequence);
    previousObserved = Number(record.observed_micros);
  }
  if (session === null) throw new Error("journal has no session line");
  if (recordCount === 0) throw new Error("journal complete prefix has no records");
  if (excludedTail !== null && endsWithNewline) {
    throw new Error(`invalid complete JSON line ${excludedTail.lineNumber} cannot be excluded`);
  }
  return { session, recordCount, lastSequence, excludedTail };
}

async function writePrefix(input, output, outputSession, scan, maxLineBytes) {
  const writer = fs.createWriteStream(output, { encoding: "utf8", flags: "wx" });
  const write = async (value) => {
    if (!writer.write(value)) await new Promise((resolve) => writer.once("drain", resolve));
  };
  await write(`${JSON.stringify({ line: "session", data: outputSession })}\n`);
  const reader = readline.createInterface({
    input: fs.createReadStream(input, { encoding: "utf8" }),
    crlfDelay: Infinity,
  });
  let lineNumber = 0;
  let recordsWritten = 0;
  for await (const line of reader) {
    lineNumber += 1;
    if (Buffer.byteLength(line) > maxLineBytes) throw new Error(`line ${lineNumber} grew`);
    if (lineNumber === scan.excludedTail?.lineNumber) break;
    if (line.trim() === "" || lineNumber === 1) continue;
    await write(`${line}\n`);
    recordsWritten += 1;
  }
  await new Promise((resolve, reject) => {
    writer.on("error", reject);
    writer.end(resolve);
  });
  if (recordsWritten !== scan.recordCount) {
    throw new Error(`wrote ${recordsWritten} records but validated ${scan.recordCount}`);
  }
}

function verifyReceipt(receipt) {
  if (Number(receipt?.schema_version) === 2) {
    if (receipt?.artifact_kind !== "gap-free-journal-segment" ||
      receipt?.generated_by !== GENERATOR || receipt?.content_sha256 !== contentHash(receipt) ||
      receipt?.policy?.source_journal_is_modified !== false ||
      receipt?.policy?.source_packet_payload_byte_arrays_are_preserved !== true ||
      receipt?.policy?.output_record_wrappers_are_resequenced !== true ||
      receipt?.policy?.selected_capture_gap_records_allowed !== false ||
      receipt?.policy?.gaps_outside_segment_are_treated_as_zero_bytes_or_events !== false ||
      Number(receipt?.selected_record_count) <= 0 ||
      Number(receipt?.selected_capture_gap_records) !== 0 ||
      Number(receipt?.source_capture_gap_records) <= 0 ||
      receipt?.authority?.gap_free_selected_segment_proven !== true ||
      receipt?.authority?.canonical_replay_conservation_proven !== false ||
      receipt?.authority?.runtime_promotion_allowed !== false ||
      receipt?.authority?.provider_rdps_credit_allowed !== false) {
      throw new Error("gap-free segment receipt is unsafe or incomplete");
    }
    for (const entry of [receipt.source, receipt.output]) {
      const actual = descriptor(entry.path);
      if (actual.bytes !== Number(entry.bytes) || actual.sha256 !== entry.sha256) {
        throw new Error(`descriptor changed for ${entry.path}`);
      }
    }
    console.log(`verified gap-free segment with ${receipt.selected_record_count} exact records`);
    return;
  }
  if (Number(receipt?.schema_version) !== 1 || receipt?.generated_by !== GENERATOR ||
    receipt?.content_sha256 !== contentHash(receipt) ||
    receipt?.policy?.source_journal_is_modified !== false ||
    receipt?.policy?.source_complete_records_are_byte_preserved !== true ||
    receipt?.policy?.explicit_capture_gaps_allowed !== false ||
    receipt?.policy?.excluded_tail_is_treated_as_zero_bytes_or_events !== false ||
    Number(receipt?.complete_record_count) <= 0 ||
    Number(receipt?.explicit_capture_gap_records) !== 0 ||
    receipt?.authority?.gap_free_complete_prefix_proven !== true ||
    receipt?.authority?.canonical_replay_conservation_proven !== false ||
    receipt?.authority?.runtime_promotion_allowed !== false ||
    receipt?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("sealed-prefix receipt is unsafe or incomplete");
  }
  for (const entry of [receipt.source, receipt.output]) {
    const actual = descriptor(entry.path);
    if (actual.bytes !== Number(entry.bytes) || actual.sha256 !== entry.sha256) {
      throw new Error(`descriptor changed for ${entry.path}`);
    }
  }
  console.log(`verified sealed prefix with ${receipt.complete_record_count} exact records`);
}

async function selfTest() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "bpsr-sealed-prefix-"));
  const input = path.join(root, "input.jsonl");
  const output = path.join(root, "output.jsonl");
  const receipt = path.join(root, "receipt.json");
  const session = {
    line: "session",
    data: {
      format_version: 1,
      capture_id: "fixture",
      started_unix_micros: 1,
      game_build: { deployment_id: "global", region_id: "global", channel: "steam", build_id: "1", executable_version: null },
      adapter: { name: "fixture", version: null },
      protocol_pack_digest: "sha256:test",
    },
  };
  const record = {
    line: "record",
    data: { sequence: 1, observed_micros: 10, wall_clock_unix_micros: 11, kind: { record: "packet", data: {} } },
  };
  fs.writeFileSync(input, `${JSON.stringify(session)}\n${JSON.stringify(record)}\n{\"line\":`, "utf8");
  await seal({ build: "1", input, output, receipt });
  const report = readJson(receipt);
  if (report.complete_record_count !== 1 || report.excluded_unterminated_tail?.bytes <= 0) {
    throw new Error("self-test did not retain the complete prefix and disclose the tail");
  }
  const segmentInput = path.join(root, "segment-input.jsonl");
  const segmentOutput = path.join(root, "segment-output.jsonl");
  const segmentReceipt = path.join(root, "segment-receipt.json");
  const records = [
    { sequence: 1, observed_micros: 1, kind: { record: "packet", data: { value: 1 } } },
    { sequence: 2, observed_micros: 2, kind: { record: "gap", data: { kind: "tcp_gap" } } },
    { sequence: 3, observed_micros: 3, kind: { record: "packet", data: { value: 3 } } },
    { sequence: 4, observed_micros: 4, kind: { record: "packet", data: { value: 4 } } },
    { sequence: 5, observed_micros: 5, kind: { record: "gap", data: { kind: "tcp_gap" } } },
  ].map((data) => JSON.stringify({ line: "record", data }));
  fs.writeFileSync(segmentInput, `${JSON.stringify(session)}\n${records.join("\n")}\n`, "utf8");
  await segment({
    build: "1", input: segmentInput, output: segmentOutput, receipt: segmentReceipt,
    "from-sequence": "3", "to-sequence": "4",
  });
  const segmentReport = readJson(segmentReceipt);
  if (segmentReport.selected_record_count !== 2 ||
    segmentReport.source_capture_gap_records !== 2 ||
    segmentReport.selected_capture_gap_records !== 0) {
    throw new Error("self-test did not disclose out-of-segment gaps correctly");
  }
  console.log("bpsr-protocol-journal-sealed-prefix self-test passed");
}

function descriptor(file) {
  const absolute = path.resolve(file);
  const bytes = fs.readFileSync(absolute);
  return { path: absolute.replaceAll("\\", "/"), bytes: bytes.length, sha256: sha256(bytes) };
}
function sha256(bytes) { return crypto.createHash("sha256").update(bytes).digest("hex"); }
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return sha256(Buffer.from(JSON.stringify(copy))); }
function readJson(file) { return JSON.parse(fs.readFileSync(path.resolve(file), "utf8")); }
function fileEndsWithNewline(file) {
  const fd = fs.openSync(file, "r");
  try {
    const size = fs.fstatSync(fd).size;
    if (size === 0) return false;
    const byte = Buffer.alloc(1);
    fs.readSync(fd, byte, 0, 1, size - 1);
    return byte[0] === 10;
  } finally { fs.closeSync(fd); }
}
function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined) throw new Error(`invalid argument near ${flag}`);
    const key = flag.slice(2);
    if (result[key] !== undefined) throw new Error(`${flag} may only be supplied once`);
    result[key] = value;
  }
  return result;
}
function required(value, key) { const result = value[key]; if (!result) throw new Error(`missing --${key}`); return result; }
function usage(code) {
  console.log("Usage:\n  node tools/bpsr-protocol-journal-sealed-prefix.mjs seal --build <id> --input <journal.jsonl> --output <sealed.jsonl> --receipt <json> [--max-line-bytes <n>]\n  node tools/bpsr-protocol-journal-sealed-prefix.mjs segment --build <id> --input <journal.jsonl> --from-sequence <n> --to-sequence <n> --output <segment.jsonl> --receipt <json> [--max-line-bytes <n>]\n  node tools/bpsr-protocol-journal-sealed-prefix.mjs verify --receipt <json>\n  node tools/bpsr-protocol-journal-sealed-prefix.mjs self-test");
  process.exit(code);
}
