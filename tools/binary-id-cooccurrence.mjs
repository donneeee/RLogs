#!/usr/bin/env node

import { closeSync, fstatSync, openSync, readFileSync, readSync, readdirSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const roots = options.roots.map(resolvePath);
const outputPath = resolvePath(options.output);
const targets = options.ids.map((value) => ({ value, patterns: integerPatterns(value) }));
const contextBytes = options.contextBytes;
const chunkBytes = 4 * 1024 * 1024;
const overlapBytes = Math.max(...targets.flatMap((target) => target.patterns.map((pattern) => pattern.bytes.length))) - 1;

const matches = [];
let filesScanned = 0;
let bytesScanned = 0;

for (const root of roots) {
  for (const filePath of walkFiles(root)) {
    const fileMatches = scanFile(filePath);
    filesScanned += 1;
    bytesScanned += statSync(filePath).size;
    if (fileMatches.length > 0) {
      matches.push({
        path: relativePath(filePath),
        size_bytes: statSync(filePath).size,
        matched_target_count: new Set(fileMatches.map((match) => match.target)).size,
        matches: fileMatches,
      });
    }
  }
}

matches.sort((left, right) => {
  if (right.matched_target_count !== left.matched_target_count) {
    return right.matched_target_count - left.matched_target_count;
  }
  return left.size_bytes - right.size_bytes || left.path.localeCompare(right.path);
});

const result = {
  schema_version: 1,
  generated_by: "tools/binary-id-cooccurrence.mjs",
  policy: {
    binary_occurrence_is_discovery_evidence_not_semantic_or_formula_proof: true,
    all_requested_integer_encodings_are_reported: true,
    context_is_bounded_and_lossless_hex: true,
  },
  inputs: {
    roots: roots.map(relativePath),
    target_ids: options.ids.map(String),
    context_bytes: contextBytes,
  },
  summary: {
    files_scanned: filesScanned,
    bytes_scanned: bytesScanned,
    files_with_matches: matches.length,
    files_with_all_targets: matches.filter((entry) => entry.matched_target_count === targets.length).length,
  },
  files: matches,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary));

function scanFile(filePath) {
  const descriptor = openSync(filePath, "r");
  const size = fstatSync(descriptor).size;
  const found = [];
  const dedupe = new Set();
  let position = 0;
  let carry = Buffer.alloc(0);

  try {
    while (position < size) {
      const count = Math.min(chunkBytes, size - position);
      const chunk = Buffer.allocUnsafe(count);
      const read = readSync(descriptor, chunk, 0, count, position);
      if (read <= 0) break;
      const block = Buffer.concat([carry, chunk.subarray(0, read)]);
      const blockBase = position - carry.length;

      for (const target of targets) {
        for (const pattern of target.patterns) {
          let cursor = 0;
          while (cursor <= block.length - pattern.bytes.length) {
            const offset = block.indexOf(pattern.bytes, cursor);
            if (offset < 0) break;
            const absoluteOffset = blockBase + offset;
            const key = `${target.value}:${pattern.encoding}:${absoluteOffset}`;
            if (!dedupe.has(key)) {
              dedupe.add(key);
              const contextStart = Math.max(0, absoluteOffset - contextBytes);
              const contextEnd = Math.min(size, absoluteOffset + pattern.bytes.length + contextBytes);
              const context = Buffer.alloc(contextEnd - contextStart);
              readSync(descriptor, context, 0, context.length, contextStart);
              found.push({
                target: String(target.value),
                encoding: pattern.encoding,
                offset: absoluteOffset,
                context_start: contextStart,
                context_hex: context.toString("hex"),
              });
            }
            cursor = offset + 1;
          }
        }
      }

      const keep = Math.min(overlapBytes, block.length);
      carry = Buffer.from(block.subarray(block.length - keep));
      position += read;
    }
  } finally {
    closeSync(descriptor);
  }

  found.sort((left, right) => left.offset - right.offset || left.target.localeCompare(right.target));
  return found;
}

function integerPatterns(value) {
  const result = [];
  if (value <= 0xffff_ffffn) {
    const little32 = Buffer.alloc(4);
    little32.writeUInt32LE(Number(value));
    result.push({ encoding: "uint32-le", bytes: little32 });
    const big32 = Buffer.alloc(4);
    big32.writeUInt32BE(Number(value));
    result.push({ encoding: "uint32-be", bytes: big32 });
  }
  const little64 = Buffer.alloc(8);
  little64.writeBigUInt64LE(value);
  result.push({ encoding: "uint64-le", bytes: little64 });
  const big64 = Buffer.alloc(8);
  big64.writeBigUInt64BE(value);
  result.push({ encoding: "uint64-be", bytes: big64 });
  result.push({ encoding: "protobuf-varint", bytes: encodeVarint(value) });
  return dedupePatterns(result);
}

function encodeVarint(value) {
  const bytes = [];
  let remaining = value;
  while (remaining >= 0x80n) {
    bytes.push(Number((remaining & 0x7fn) | 0x80n));
    remaining >>= 7n;
  }
  bytes.push(Number(remaining));
  return Buffer.from(bytes);
}

function dedupePatterns(patterns) {
  const seen = new Set();
  return patterns.filter((pattern) => {
    const key = pattern.bytes.toString("hex");
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function* walkFiles(root) {
  const rootStat = statSync(root);
  if (rootStat.isFile()) {
    yield root;
    return;
  }
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    const entries = readdirSync(current, { withFileTypes: true })
      .sort((left, right) => right.name.localeCompare(left.name));
    for (const entry of entries) {
      const child = path.join(current, entry.name);
      if (entry.isDirectory()) stack.push(child);
      else if (entry.isFile()) yield child;
    }
  }
}

function resolvePath(input) {
  return path.isAbsolute(input) ? input : path.resolve(repoRoot, input);
}

function relativePath(input) {
  return path.relative(repoRoot, input).replaceAll("\\", "/");
}

function parseArgs(argv) {
  const result = { roots: [], ids: [], output: "", contextBytes: 64 };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    const value = argv[index + 1];
    if (["--root", "--id", "--output", "--context-bytes"].includes(argument) && !value) {
      throw new Error(`Missing value for ${argument}`);
    }
    if (argument === "--root") result.roots.push(value), index += 1;
    else if (argument === "--id") result.ids.push(BigInt(value)), index += 1;
    else if (argument === "--output") result.output = value, index += 1;
    else if (argument === "--context-bytes") result.contextBytes = Number(value), index += 1;
    else throw new Error(`Unknown argument: ${argument}`);
  }
  if (result.roots.length === 0 || result.ids.length === 0 || !result.output) {
    throw new Error("Usage: node tools/binary-id-cooccurrence.mjs --root <path> --id <integer> [--id <integer>...] --output <json>");
  }
  if (!Number.isInteger(result.contextBytes) || result.contextBytes < 0 || result.contextBytes > 4096) {
    throw new Error("--context-bytes must be an integer from 0 through 4096");
  }
  return result;
}
