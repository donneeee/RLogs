#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  createReadStream,
  existsSync,
  openSync,
  closeSync,
  readSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-protocol-journal-closure-inventory.mjs";
const DEFAULT_MAX_LINE_BYTES = 128 * 1024 * 1024;

const [command = "help", ...argv] = process.argv.slice(2);
const options = parseArgs(argv);

if (command === "build") await build(options);
else if (command === "verify") verify(readJson(required(options, "input")));
else usage(command === "help" ? 0 : 1);

async function build(values) {
  const buildId = required(values, "build");
  const journalsRoot = path.resolve(required(values, "journals-root"));
  const output = path.resolve(required(values, "output"));
  const maxLineBytes = Number(values["max-line-bytes"] ?? DEFAULT_MAX_LINE_BYTES);
  if (!/^\d+$/.test(buildId)) throw new Error("--build must contain only ASCII digits");
  if (!statSync(journalsRoot).isDirectory()) throw new Error("--journals-root is not a directory");
  if (existsSync(output)) throw new Error(`refusing to overwrite ${output}`);
  if (!Number.isSafeInteger(maxLineBytes) || maxLineBytes <= 0) {
    throw new Error("--max-line-bytes must be a positive safe integer");
  }

  const files = readdirSync(journalsRoot, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".protocol.jsonl"))
    .map((entry) => path.join(journalsRoot, entry.name))
    .sort();
  const journals = [];
  for (const file of files) {
    const receipt = await scanJournal(file, buildId, maxLineBytes);
    journals.push(receipt);
    console.error(
      `${path.basename(file)}: records=${receipt.complete_record_count} gaps=${receipt.capture_gap_records} ` +
      `strict=${receipt.strict_full_replay_candidate} sealed-prefix=${receipt.sealed_prefix_replay_candidate}`,
    );
  }

  const strict = journals.filter((entry) => entry.strict_full_replay_candidate);
  const sealedPrefix = journals.filter((entry) => entry.sealed_prefix_replay_candidate);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: buildId,
    policy: {
      input_journals_are_read_only: true,
      scanning_is_streaming_one_file_at_a_time: true,
      missing_or_invalid_records_are_not_synthesized: true,
      truncated_tails_are_not_treated_as_complete: true,
      gap_free_transport_does_not_prove_closed_encounter_scope: true,
      protocol_or_formula_authority_granted: false,
      provider_rdps_credit_allowed: false,
    },
    journals_root: journalsRoot.replaceAll("\\", "/"),
    summary: {
      journal_count: journals.length,
      total_bytes: journals.reduce((sum, entry) => sum + entry.bytes, 0),
      total_complete_records: journals.reduce((sum, entry) => sum + entry.complete_record_count, 0),
      total_capture_gap_records: journals.reduce((sum, entry) => sum + entry.capture_gap_records, 0),
      strict_full_replay_candidate_count: strict.length,
      sealed_prefix_replay_candidate_count: sealedPrefix.length,
      strict_full_replay_candidates: strict.map((entry) => entry.path),
      sealed_prefix_replay_candidates: sealedPrefix.map((entry) => entry.path),
      closed_encounter_scope_proven: false,
      runtime_promotion_allowed: false,
    },
    journals,
  };
  verify(report);
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { encoding: "utf8", flag: "wx" });
  console.log(JSON.stringify(report.summary, null, 2));
}

async function scanJournal(file, buildId, maxLineBytes) {
  const hash = createHash("sha256");
  const stream = createReadStream(file, { encoding: "utf8" });
  stream.on("data", (chunk) => hash.update(chunk));
  const reader = createInterface({ input: stream, crlfDelay: Infinity });
  let session = null;
  let lineNumber = 0;
  let completeRecordCount = 0;
  let captureGapRecords = 0;
  let firstSequence = null;
  let lastSequence = null;
  let sequenceContiguous = true;
  let observedMicrosNondecreasing = true;
  let lastObservedMicros = null;
  let invalidLine = null;
  let nonblankAfterInvalid = false;
  const recordKindCounts = {};
  const gapKindCounts = {};

  for await (const line of reader) {
    lineNumber += 1;
    if (!line.trim()) continue;
    const lineBytes = Buffer.byteLength(line);
    if (lineBytes > maxLineBytes) throw new Error(`${file}: line ${lineNumber} exceeds max line bytes`);
    if (invalidLine !== null) {
      nonblankAfterInvalid = true;
      continue;
    }
    let parsed;
    try {
      parsed = JSON.parse(line);
    } catch (error) {
      invalidLine = {
        line_number: lineNumber,
        bytes: lineBytes,
        sha256: createHash("sha256").update(line).digest("hex"),
        error: String(error).slice(0, 300),
      };
      continue;
    }
    if (session === null) {
      if (parsed?.line !== "session" || typeof parsed?.data !== "object") {
        throw new Error(`${file}: first nonblank line is not a session`);
      }
      session = parsed.data;
      continue;
    }
    if (parsed?.line !== "record" || typeof parsed?.data !== "object") {
      throw new Error(`${file}: line ${lineNumber} is not a record`);
    }
    completeRecordCount += 1;
    const sequence = Number(parsed.data.sequence);
    if (!Number.isSafeInteger(sequence) || sequence <= 0) sequenceContiguous = false;
    if (firstSequence === null) firstSequence = sequence;
    if (lastSequence !== null && sequence !== lastSequence + 1) sequenceContiguous = false;
    lastSequence = sequence;
    const observedMicros = Number(parsed.data.observed_micros);
    if (!Number.isSafeInteger(observedMicros) ||
        (lastObservedMicros !== null && observedMicros < lastObservedMicros)) {
      observedMicrosNondecreasing = false;
    }
    lastObservedMicros = observedMicros;
    const recordKind = String(parsed.data.kind?.record ?? "missing");
    recordKindCounts[recordKind] = (recordKindCounts[recordKind] ?? 0) + 1;
    if (recordKind === "gap") {
      captureGapRecords += 1;
      const gapKind = String(parsed.data.kind?.data?.kind ?? "unknown");
      gapKindCounts[gapKind] = (gapKindCounts[gapKind] ?? 0) + 1;
    }
  }

  const endedWithNewline = fileEndsWithNewline(file);
  const buildMatches = String(session?.game_build?.build_id ?? "") === buildId;
  const finalInvalidTailOnly = invalidLine !== null && !nonblankAfterInvalid && !endedWithNewline;
  const commonSafe = buildMatches && completeRecordCount > 0 && captureGapRecords === 0 &&
    sequenceContiguous && observedMicrosNondecreasing && firstSequence === 1;
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: hash.digest("hex"),
    capture_id: session?.capture_id ?? null,
    deployment_id: session?.game_build?.deployment_id ?? null,
    channel: session?.game_build?.channel ?? null,
    build_id: session?.game_build?.build_id ?? null,
    source_protocol_pack_digest: session?.protocol_pack_digest ?? null,
    complete_record_count: completeRecordCount,
    first_sequence: firstSequence,
    last_sequence: lastSequence,
    capture_gap_records: captureGapRecords,
    gap_kind_counts: gapKindCounts,
    record_kind_counts: recordKindCounts,
    sequence_contiguous_from_one: sequenceContiguous && firstSequence === 1,
    observed_micros_nondecreasing: observedMicrosNondecreasing,
    ended_with_newline: endedWithNewline,
    invalid_line: invalidLine,
    nonblank_after_invalid_line: nonblankAfterInvalid,
    strict_full_replay_candidate: commonSafe && invalidLine === null && endedWithNewline,
    sealed_prefix_replay_candidate: commonSafe && finalInvalidTailOnly,
    closed_encounter_scope_proven: false,
  };
}

function verify(report) {
  if (report.schema_version !== SCHEMA_VERSION || report.generated_by !== GENERATED_BY) {
    throw new Error("unsupported closure inventory schema or generator");
  }
  if (report.policy?.scanning_is_streaming_one_file_at_a_time !== true ||
      report.policy?.gap_free_transport_does_not_prove_closed_encounter_scope !== true ||
      report.policy?.protocol_or_formula_authority_granted !== false ||
      report.policy?.provider_rdps_credit_allowed !== false ||
      report.summary?.closed_encounter_scope_proven !== false ||
      report.summary?.runtime_promotion_allowed !== false) {
    throw new Error("unsafe closure inventory policy");
  }
  const journals = report.journals ?? [];
  for (const entry of journals) {
    const commonSafe = entry.build_id === report.game_build && entry.complete_record_count > 0 &&
      entry.capture_gap_records === 0 && entry.sequence_contiguous_from_one === true &&
      entry.observed_micros_nondecreasing === true;
    const strict = commonSafe && entry.invalid_line === null && entry.ended_with_newline === true;
    const sealed = commonSafe && entry.invalid_line !== null &&
      entry.nonblank_after_invalid_line === false && entry.ended_with_newline === false;
    if (entry.strict_full_replay_candidate !== strict || entry.sealed_prefix_replay_candidate !== sealed ||
        entry.closed_encounter_scope_proven !== false) {
      throw new Error(`candidate classification mismatch for ${entry.path}`);
    }
  }
  const strictCount = journals.filter((entry) => entry.strict_full_replay_candidate).length;
  const sealedCount = journals.filter((entry) => entry.sealed_prefix_replay_candidate).length;
  if (report.summary?.journal_count !== journals.length ||
      report.summary?.strict_full_replay_candidate_count !== strictCount ||
      report.summary?.sealed_prefix_replay_candidate_count !== sealedCount) {
    throw new Error("closure inventory summary mismatch");
  }
  return report;
}

function fileEndsWithNewline(file) {
  const handle = openSync(file, "r");
  try {
    const size = statSync(file).size;
    if (size === 0) return false;
    const byte = Buffer.alloc(1);
    readSync(handle, byte, 0, 1, size - 1);
    return byte[0] === 10;
  } finally {
    closeSync(handle);
  }
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined) throw new Error(`invalid argument near ${flag}`);
    result[flag.slice(2)] = value;
  }
  return result;
}

function required(values, key) {
  if (!values[key]) throw new Error(`missing --${key}`);
  return String(values[key]);
}

function readJson(file) {
  return JSON.parse(readFileSync(path.resolve(file), "utf8").replace(/^\uFEFF/, ""));
}

function usage(exitCode) {
  console.log(
    "Usage:\n" +
    "  node tools/bpsr-protocol-journal-closure-inventory.mjs build --build <id> --journals-root <dir> --output <json>\n" +
    "  node tools/bpsr-protocol-journal-closure-inventory.mjs verify --input <json>",
  );
  process.exit(exitCode);
}
