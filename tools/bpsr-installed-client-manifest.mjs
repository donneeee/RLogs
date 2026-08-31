#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const buildRoot = resolvePath(required(options, "buildRoot"));
  const physicalRoot = resolvePath(options.physicalRoot ?? path.join(buildRoot, "physical", "files"));
  const distributionPath = resolvePath(options.distribution ?? path.join(buildRoot, "steam-distribution-snapshot.v1.json"));
  const outputPath = resolvePath(options.output ?? path.join(buildRoot, "installed-client-file-manifest.v1.json"));
  const depotManifestPath = options.depotManifest ? resolvePath(options.depotManifest) : undefined;
  const distribution = readJson(distributionPath);
  const manifest = buildManifest({ physicalRoot, distribution, depotManifestPath });
  writeJson(outputPath, manifest);
  console.log(`Installed client build ${manifest.gameBuild}: ${manifest.coverage.physicalFilesDiscovered}/${manifest.coverage.physicalFilesHashed} files accounted for.`);
  console.log(`Depot-authored bytes: ${manifest.coverage.depotAuthoredBytes}/${manifest.coverage.installedDepotBytesExpected}.`);
  console.log(`Client-generated volatile evidence: ${manifest.coverage.clientGeneratedVolatileFiles} file(s), ${manifest.coverage.clientGeneratedVolatileBytes} bytes.`);
  console.log(`Wrote ${relativeRepo(outputPath)}`);
}

function verify(options) {
  const manifestPath = resolvePath(required(options, "manifest"));
  const manifest = readJson(manifestPath);
  const buildRoot = resolvePath(options.buildRoot ?? path.dirname(manifestPath));
  const physicalRoot = resolvePath(options.physicalRoot ?? path.join(buildRoot, "physical", "files"));
  const distributionPath = resolvePath(options.distribution ?? path.join(buildRoot, "steam-distribution-snapshot.v1.json"));
  const depotManifestPath = options.depotManifest ? resolvePath(options.depotManifest) : undefined;
  const rebuilt = buildManifest({ physicalRoot, distribution: readJson(distributionPath), depotManifestPath });
  assert(JSON.stringify(manifest) === JSON.stringify(rebuilt), "Installed-client manifest does not match the current physical evidence");
  console.log(`Verified ${relativeRepo(manifestPath)} (${manifest.coverage.physicalFilesHashed} files).`);
}

function buildManifest({ physicalRoot, distribution, depotManifestPath }) {
  assert(distribution?.app?.buildId, "Distribution snapshot has no build ID");
  assert(Array.isArray(distribution.installedDepots) && distribution.installedDepots.length > 0, "Distribution snapshot has no installed depots");
  assert(existsSync(physicalRoot) && statSync(physicalRoot).isDirectory(), `Missing physical inventory: ${physicalRoot}`);

  const depots = distribution.installedDepots.map((depot) => ({
    depotId: String(depot.depotId),
    manifestId: String(depot.manifestId),
    sizeBytes: Number(depot.sizeBytes),
  })).sort((left, right) => left.depotId.localeCompare(right.depotId));
  const expectedDepotBytes = depots.reduce((sum, depot) => sum + depot.sizeBytes, 0);
  const seen = new Set();
  const files = [];
  const familyFiles = readdirSync(physicalRoot, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .sort((left, right) => left.name.localeCompare(right.name));
  assert(familyFiles.length > 0, "Physical inventory has no family files");

  for (const familyFile of familyFiles) {
    const family = familyFile.name.slice(0, -5);
    const rows = readJson(path.join(physicalRoot, familyFile.name));
    assert(Array.isArray(rows), `Physical family ${family} is not an array`);
    for (const row of rows) {
      const relativePath = normalizeRelative(row.relative_path);
      assert(relativePath.length > 0, `Physical family ${family} has an empty relative path`);
      assert(!seen.has(relativePath), `Duplicate physical path: ${relativePath}`);
      assert(row.stable_during_scan === true, `Unstable physical evidence: ${relativePath}`);
      const sha256 = normalizeHash(row.sha256);
      assert(/^[0-9a-f]{64}$/.test(sha256), `Invalid SHA-256 for ${relativePath}`);
      const bytes = Number(row.bytes);
      assert(Number.isSafeInteger(bytes) && bytes >= 0, `Invalid byte count for ${relativePath}`);
      seen.add(relativePath);
      const origin = family === "volatile_private_log" ? "client-generated-volatile" : "steam-depot-authored";
      files.push({
        relativePath,
        family,
        origin,
        bytes,
        sha256,
        extension: String(row.extension ?? ""),
        signature: String(row.signature ?? ""),
      });
    }
  }
  files.sort((left, right) => left.relativePath.localeCompare(right.relativePath));

  const depotFiles = files.filter((file) => file.origin === "steam-depot-authored");
  const volatileFiles = files.filter((file) => file.origin === "client-generated-volatile");
  const depotBytes = sumBytes(depotFiles);
  const volatileBytes = sumBytes(volatileFiles);
  assert(depotBytes === expectedDepotBytes, `Depot-authored bytes do not match installed depot size: expected ${expectedDepotBytes}, received ${depotBytes}`);

  const familyMap = new Map();
  for (const file of files) {
    const current = familyMap.get(file.family) ?? { family: file.family, origin: file.origin, files: 0, bytes: 0 };
    assert(current.origin === file.origin, `Physical family ${file.family} mixes file origins`);
    current.files += 1;
    current.bytes += file.bytes;
    familyMap.set(file.family, current);
  }
  const families = [...familyMap.values()].sort((left, right) => left.family.localeCompare(right.family));
  const cachedDepotManifest = depotManifestPath
    ? buildDepotManifestEvidence(depotManifestPath, depots)
    : null;

  return {
    schemaVersion: 1,
    generatedBy: "tools/bpsr-installed-client-manifest.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    gameBuild: String(distribution.app.buildId),
    distributionIdentity: {
      appId: String(distribution.app.appId),
      depots,
      routingFingerprintSha256: String(distribution.routingFingerprintSha256),
    },
    authority: {
      steamDb: "public-change-history-and-depot-manifest-index-only",
      localSteamAppManifest: "installed-build-and-depot-identity",
      cachedSteamDepotManifest: "optional-local-manifest-artifact-identity",
      localPhysicalSha256: "installed-file-content-proof",
      extractedSemanticsAndPacketReplay: "mechanics-and-runtime-proof",
    },
    policy: {
      absolutePathsStored: false,
      everyInstalledFileRetained: true,
      clientGeneratedVolatileFilesSeparatedNotHidden: true,
      steamDbNeverUsedAsMechanicsAuthority: true,
      changedDepotFilesRouteTargetedRescans: true,
      unresolvedSemanticEvidenceRemainsVisible: true,
    },
    coverage: {
      complete: files.length === seen.size && depotBytes === expectedDepotBytes,
      physicalFilesDiscovered: files.length,
      physicalFilesHashed: files.length,
      physicalBytesHashed: depotBytes + volatileBytes,
      depotAuthoredFiles: depotFiles.length,
      depotAuthoredBytes: depotBytes,
      installedDepotBytesExpected: expectedDepotBytes,
      clientGeneratedVolatileFiles: volatileFiles.length,
      clientGeneratedVolatileBytes: volatileBytes,
      silentOmissions: 0,
      unstableFiles: 0,
    },
    aggregateSha256: aggregateFiles(files),
    cachedDepotManifest,
    families,
    files,
  };
}

function buildDepotManifestEvidence(filePath, depots) {
  assert(existsSync(filePath) && statSync(filePath).isFile(), `Missing cached depot manifest: ${filePath}`);
  const fileName = path.basename(filePath);
  const match = /^(\d+)_(\d+)\.manifest$/i.exec(fileName);
  assert(match, `Cached depot manifest name does not encode depot and manifest IDs: ${fileName}`);
  assert(depots.some((depot) => depot.depotId === match[1] && depot.manifestId === match[2]), `Cached depot manifest ${fileName} does not match the installed distribution`);
  const content = readFileSync(filePath);
  return {
    depotId: match[1],
    manifestId: match[2],
    bytes: content.length,
    sha256: hash(content),
    fileName,
  };
}

function aggregateFiles(files) {
  const digest = createHash("sha256");
  for (const file of files) {
    digest.update(`${file.relativePath}\0${file.family}\0${file.origin}\0${file.bytes}\0${file.sha256}\n`, "utf8");
  }
  return digest.digest("hex");
}

function sumBytes(files) {
  return files.reduce((sum, file) => sum + file.bytes, 0);
}

function normalizeRelative(value) {
  const normalized = String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, "");
  assert(!path.posix.isAbsolute(normalized), `Absolute physical path is forbidden: ${normalized}`);
  assert(!/^[a-z]:\//i.test(normalized), `Drive-qualified physical path is forbidden: ${normalized}`);
  assert(!normalized.split("/").includes(".."), `Parent traversal is forbidden: ${normalized}`);
  return normalized;
}

function normalizeHash(value) { return String(value ?? "").replace(/^sha256:/i, "").toLowerCase(); }
function hash(value) { return createHash("sha256").update(value).digest("hex"); }
function readJson(filePath) { return JSON.parse(readFileSync(filePath, "utf8")); }
function writeJson(filePath, value) { mkdirSync(path.dirname(filePath), { recursive: true }); writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function resolvePath(value) { return path.isAbsolute(value) ? value : path.resolve(repoRoot, value); }
function relativeRepo(value) { return path.relative(repoRoot, value).replaceAll("\\", "/"); }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${toKebab(key)}`); return value[key]; }
function toKebab(value) { return value.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`); }
function assert(condition, message) { if (!condition) throw new Error(message); }

function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const next = args[index + 1];
    if (!next || next.startsWith("--")) throw new Error(`Missing value for ${token}`);
    output[token.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = next;
    index += 1;
  }
  return output;
}

function selfTest() {
  const rows = [
    { relativePath: "a", family: "x", origin: "steam-depot-authored", bytes: 1, sha256: "0".repeat(64) },
    { relativePath: "b", family: "y", origin: "client-generated-volatile", bytes: 2, sha256: "1".repeat(64) },
  ];
  assert(sumBytes(rows) === 3, "Byte summation failed");
  assert(aggregateFiles(rows) === aggregateFiles([...rows]), "Aggregate hashing is unstable");
  assert(normalizeRelative("a\\b") === "a/b", "Relative path normalization failed");
  console.log("bpsr-installed-client-manifest self-test passed");
}

function usage(exitCode) {
  console.log("Usage:");
  console.log("  node tools/bpsr-installed-client-manifest.mjs generate --build-root <directory> [--physical-root <directory>] [--distribution <json>] [--depot-manifest <binary manifest>] [--output <json>]");
  console.log("  node tools/bpsr-installed-client-manifest.mjs verify --manifest <json> [--build-root <directory>] [--physical-root <directory>] [--distribution <json>] [--depot-manifest <binary manifest>]");
  console.log("  node tools/bpsr-installed-client-manifest.mjs self-test");
  process.exit(exitCode);
}
