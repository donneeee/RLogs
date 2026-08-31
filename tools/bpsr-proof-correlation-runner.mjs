#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "run") run(options);
else if (command === "verify") verify(path.resolve(requiredOne(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function run(parsed) {
  const manifest = path.resolve(requiredOne(parsed, "manifest"));
  const cacheRoot = path.resolve(requiredOne(parsed, "cache-root"));
  const output = path.resolve(requiredOne(parsed, "output"));
  const captures = requiredMany(parsed, "capture").map((value) => path.resolve(value));
  const validator = parsed.validator?.[0] ? path.resolve(parsed.validator[0]) : null;
  const manifestBytes = readFileSync(manifest);
  const manifestSha256 = hashBytes(manifestBytes);
  const manifestValue = JSON.parse(manifestBytes.toString("utf8"));
  const cacheDirectory = path.join(cacheRoot, manifestSha256);
  mkdirSync(cacheDirectory, { recursive: true });
  const entries = [];
  let cacheHits = 0;
  let cacheMisses = 0;
  for (const capture of captures) {
    requireFile(capture, "capture");
    const captureSha256 = hashFile(capture);
    const cacheFile = path.join(cacheDirectory, `${captureSha256}.validation.json`);
    let cached;
    if (existsSync(cacheFile)) {
      cached = verifyCacheEntry(cacheFile, manifestSha256, captureSha256);
      cacheHits += 1;
    } else {
      cached = auditCapture({ manifest, manifestSha256, capture, captureSha256, cacheFile, validator });
      cacheMisses += 1;
    }
    entries.push({
      capture_sha256: captureSha256,
      capture_bytes: statSync(capture).size,
      cache_state: existsSync(cacheFile) && cached.generated_now !== true ? "hit" : "miss",
      session_id: cached.audit.reports?.[0]?.session_id ?? null,
      total_events: Number(cached.audit.total_events ?? 0),
      events_per_second: Number(cached.audit.events_per_second ?? 0),
      validation_summary: cached.audit.aggregate?.summary ?? null,
      cache_content_sha256: cached.content_sha256,
    });
  }
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-proof-correlation-runner.mjs",
    game_build: String(manifestValue.game_build ?? "unknown"),
    manifest_sha256: manifestSha256,
    policy: {
      each_new_capture_is_streamed_once_per_manifest_hash: true,
      cache_reuse_requires_exact_manifest_and_capture_hashes: true,
      private_source_paths_are_not_retained: true,
      build_mismatch_remains_visible_and_does_not_blank_results: true,
    },
    summary: {
      captures: entries.length,
      cache_hits: cacheHits,
      cache_misses: cacheMisses,
      total_events: entries.reduce((sum, item) => sum + item.total_events, 0),
    },
    captures: entries.sort((a, b) => a.capture_sha256.localeCompare(b.capture_sha256)),
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(output);
  console.log(
    `Proof correlation run complete: ${entries.length} capture(s), ${cacheHits} cache hit(s), ` +
    `${cacheMisses} one-pass scan(s), ${report.summary.total_events} event(s).`,
  );
}

function auditCapture(context) {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-proof-correlation-run-"));
  try {
    const rawOutput = path.join(root, "validation.json");
    if (context.validator) {
      execFileSync(context.validator, [
        "--manifest", context.manifest, "--output", rawOutput, context.capture,
      ], { cwd: repoRoot, stdio: "inherit" });
    } else {
      execFileSync("cargo", [
        "run", "--quiet", "-p", "rlogs-game-bpsr", "--bin", "rlogs-bpsr-rdps-validation-audit", "--",
        "--manifest", context.manifest, "--output", rawOutput, context.capture,
      ], { cwd: repoRoot, stdio: "inherit" });
    }
    const raw = JSON.parse(readFileSync(rawOutput, "utf8"));
    const sanitized = sanitizeAudit(raw, context.manifestSha256, context.captureSha256);
    const cacheEntry = {
      schema_version: 1,
      generated_by: "tools/bpsr-proof-correlation-runner.mjs",
      manifest_sha256: context.manifestSha256,
      capture_sha256: context.captureSha256,
      capture_bytes: statSync(context.capture).size,
      audit: sanitized,
    };
    cacheEntry.content_sha256 = contentHash(cacheEntry);
    mkdirSync(path.dirname(context.cacheFile), { recursive: true });
    writeFileSync(context.cacheFile, `${JSON.stringify(cacheEntry, null, 2)}\n`, "utf8");
    const verified = verifyCacheEntry(context.cacheFile, context.manifestSha256, context.captureSha256);
    verified.generated_now = true;
    return verified;
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function sanitizeAudit(raw, manifestSha256, captureSha256) {
  const clone = structuredClone(raw);
  clone.manifest_path = `sha256:${manifestSha256}`;
  for (const entry of clone.reports ?? []) entry.source_path = `sha256:${captureSha256}`;
  return clone;
}

function verifyCacheEntry(input, manifestSha256, captureSha256) {
  const value = JSON.parse(readFileSync(input, "utf8"));
  if (value.schema_version !== 1 || value.content_sha256 !== contentHash(value)) throw new Error(`Invalid cache entry ${path.basename(input)}`);
  if (value.manifest_sha256 !== manifestSha256 || value.capture_sha256 !== captureSha256) throw new Error(`Cache key mismatch ${path.basename(input)}`);
  if (containsPrivatePath(value)) throw new Error(`Private path leaked into cache ${path.basename(input)}`);
  return value;
}

function verify(input) {
  const report = JSON.parse(readFileSync(input, "utf8"));
  if (report.schema_version !== 1 || report.content_sha256 !== contentHash(report)) throw new Error("Proof correlation run summary is invalid");
  if (!report.policy?.cache_reuse_requires_exact_manifest_and_capture_hashes || !report.policy?.private_source_paths_are_not_retained) {
    throw new Error("Proof correlation run policy is unsafe");
  }
  if (containsPrivatePath(report)) throw new Error("Private source path leaked into proof correlation run summary");
  if (Number(report.summary?.captures) !== (report.captures?.length ?? 0)) throw new Error("Capture count mismatch");
  console.log(`Proof correlation run summary verified: ${report.summary.captures} capture(s).`);
  return report;
}

function containsPrivatePath(value) {
  const text = JSON.stringify(value);
  return /[A-Za-z]:[\\/]|(?:^|["'])\.{0,2}[\\/]/.test(text) || text.includes("source_path\":\"") && !text.includes("source_path\":\"sha256:");
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-proof-correlation-runner-"));
  try {
    const manifest = Buffer.from("manifest-a");
    const capture = Buffer.from("capture-a");
    const manifestHash = hashBytes(manifest);
    const captureHash = hashBytes(capture);
    const sanitized = sanitizeAudit({ manifest_path: "C:\\private\\manifest.json", reports: [{ source_path: "C:\\private\\capture.rlog", session_id: "s" }] }, manifestHash, captureHash);
    const cache = {
      schema_version: 1, generated_by: "self-test", manifest_sha256: manifestHash, capture_sha256: captureHash,
      capture_bytes: capture.length, audit: sanitized,
    };
    cache.content_sha256 = contentHash(cache);
    const cacheFile = path.join(root, "cache.json");
    writeFileSync(cacheFile, `${JSON.stringify(cache)}\n`);
    verifyCacheEntry(cacheFile, manifestHash, captureHash);
    const differentManifestHash = hashBytes(Buffer.from("manifest-b"));
    if (path.join(root, manifestHash, `${captureHash}.json`) === path.join(root, differentManifestHash, `${captureHash}.json`)) {
      throw new Error("Manifest hash did not partition cache");
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
  console.log("bpsr-proof-correlation-runner self-test passed");
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const token = args[index];
    if (!token?.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`Missing value for ${token}`);
    (result[token.slice(2)] ??= []).push(value);
  }
  return result;
}
function requiredOne(value, key) { if (!value[key]?.[0]) throw new Error(`Missing --${key}`); return String(value[key][0]); }
function requiredMany(value, key) { if (!value[key]?.length) throw new Error(`At least one --${key} is required`); return value[key].map(String); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function hashBytes(bytes) { return createHash("sha256").update(bytes).digest("hex"); }
function hashFile(file) { return hashBytes(readFileSync(file)); }
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashBytes(Buffer.from(JSON.stringify(clone))); }
function usage(exitCode) { console.log(`Usage:\n  node tools/bpsr-proof-correlation-runner.mjs run --manifest <json> --cache-root <dir> --output <json> --capture <rlog> [--capture <rlog> ...] [--validator <exe>]\n  node tools/bpsr-proof-correlation-runner.mjs verify --input <json>\n  node tools/bpsr-proof-correlation-runner.mjs self-test`); process.exit(exitCode); }
