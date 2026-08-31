#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync,
  createReadStream,
  existsSync,
  openSync,
  readFileSync,
  readSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-skill-logic-bundle-proof.mjs";
const EXPECTED_BUILD = "24687926";
const EXPECTED_DEPLOYMENT = "global";
const EXPECTED_CHANNEL = "steam";
const TARGET_ADDRESS = "bin/datas/logic_skill_bullet";
const TARGET_ADDRESS_HASH = 3_876_661_724;
const TARGET_BUNDLE_HASH = 520_491_686;
const EXPECTED_FILES = new Map([
  [
    "bpsr/BPSR_STEAM_Data/StreamingAssets/container/m0.pkg",
    {
      bytes: 1_274_250_093,
      sha256: "4647e2da3e2dd4c58d1dcf9df40079525f54210f579bfccfc21f1a0757e26078",
    },
  ],
  [
    "bpsr/BPSR_STEAM_Data/StreamingAssets/container/meta.pkg",
    {
      bytes: 3_389_543,
      sha256: "3591d33314ddec722b72d01006c41e4e0e15d63faa8e78940a6a21775efff80e",
    },
  ],
]);

function fail(message) {
  throw new Error(message);
}

function take(values, flag) {
  const index = values.indexOf(flag);
  if (index < 0 || index + 1 >= values.length) fail(`${flag} requires a value`);
  const value = values[index + 1];
  values.splice(index, 2);
  return value;
}

function parseArgs(argv) {
  const values = [...argv];
  const command = values.shift();
  if (command === "analyze") {
    const result = {
      command,
      build: take(values, "--build"),
      gameRoot: path.resolve(take(values, "--game-root")),
      identity: path.resolve(take(values, "--identity")),
      installedManifest: path.resolve(take(values, "--installed-manifest")),
      outputBundle: path.resolve(take(values, "--output-bundle")),
      output: path.resolve(take(values, "--output")),
    };
    if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
    return result;
  }
  if (command === "verify") {
    const result = {
      command,
      input: path.resolve(take(values, "--input")),
      bundle: path.resolve(take(values, "--bundle")),
    };
    if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
    return result;
  }
  fail("usage: analyze --build <id> --game-root <dir> --identity <json> --installed-manifest <json> --output-bundle <ab> --output <json> | verify --input <json> --bundle <ab>");
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function hashFile(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file, { highWaterMark: 4 * 1024 * 1024 })) {
    hash.update(chunk);
  }
  return hash.digest("hex");
}

function relativeToFile(gameRoot, relativePath) {
  return path.join(gameRoot, ...relativePath.split("/"));
}

function manifestEntry(manifest, relativePath) {
  const entry = manifest.files?.find((candidate) => candidate.relativePath === relativePath);
  if (!entry) fail(`installed manifest is missing ${relativePath}`);
  return entry;
}

async function verifyExactFile(gameRoot, manifest, relativePath) {
  const expected = EXPECTED_FILES.get(relativePath);
  const entry = manifestEntry(manifest, relativePath);
  const file = relativeToFile(gameRoot, relativePath);
  if (!existsSync(file)) fail(`installed client is missing ${relativePath}`);
  const bytes = statSync(file).size;
  if (
    bytes !== expected.bytes ||
    Number(entry.bytes) !== expected.bytes ||
    String(entry.sha256) !== expected.sha256
  ) {
    fail(`${relativePath} is not the reviewed exact-build manifest entry`);
  }
  const observed = await hashFile(file);
  if (observed !== expected.sha256) fail(`${relativePath} content digest changed`);
  return { relative_path: relativePath, bytes, sha256: observed, file };
}

function readMetaEntries(metaBytes) {
  let offset = 0;
  const requireBytes = (count) => {
    if (offset + count > metaBytes.length) fail("meta.pkg ended early");
  };
  const i32 = () => {
    requireBytes(4);
    const value = metaBytes.readInt32LE(offset);
    offset += 4;
    return value;
  };
  const u32 = () => {
    requireBytes(4);
    const value = metaBytes.readUInt32LE(offset);
    offset += 4;
    return value;
  };
  const u16 = () => {
    requireBytes(2);
    const value = metaBytes.readUInt16LE(offset);
    offset += 2;
    return value;
  };
  i32();
  i32();
  i32();
  requireBytes(8);
  offset += 8;
  u32();
  const headerCount = u16();
  if (headerCount > 100_000) fail(`invalid meta.pkg header count ${headerCount}`);
  requireBytes(16 * headerCount);
  offset += 16 * headerCount;
  const entries = [];
  readSection(i32());
  readSection(i32());
  return entries;

  function readSection(count) {
    if (count < 0 || count > 10_000_000) fail(`invalid meta.pkg entry count ${count}`);
    for (let index = 0; index < count; index += 1) {
      const key = u32();
      requireBytes(1);
      const type = metaBytes[offset];
      offset += 1;
      const packageIndex = u16();
      const entryOffset = i32();
      const length = i32();
      if (entryOffset < 0 || length < 0) fail(`invalid meta.pkg entry ${key}`);
      entries.push({ key, type, package_index: packageIndex, offset: entryOffset, bytes: length });
    }
  }
}

function findCatalogIdentity(m0File) {
  const prefix = Buffer.from(`address:${TARGET_ADDRESS} ->>>> hash:`);
  const fd = openSync(m0File, "r");
  const buffer = Buffer.allocUnsafe(8 * 1024 * 1024);
  let carry = Buffer.alloc(0);
  let fileOffset = 0;
  const matches = [];
  try {
    while (true) {
      const bytesRead = readSync(fd, buffer, 0, buffer.length, fileOffset);
      if (!bytesRead) break;
      const chunk = buffer.subarray(0, bytesRead);
      const combined = carry.length ? Buffer.concat([carry, chunk]) : chunk;
      let searchOffset = 0;
      while (searchOffset < combined.length) {
        const found = combined.indexOf(prefix, searchOffset);
        if (found < 0) break;
        let end = found;
        const maximum = Math.min(combined.length, found + 512);
        while (end < maximum && combined[end] !== 0 && combined[end] !== 10 && combined[end] !== 13) end += 1;
        matches.push(combined.subarray(found, end).toString("utf8"));
        searchOffset = Math.max(end + 1, found + prefix.length);
      }
      carry = Buffer.from(combined.subarray(Math.max(0, combined.length - 1024)));
      fileOffset += bytesRead;
    }
  } finally {
    closeSync(fd);
  }
  const unique = [...new Set(matches)];
  if (unique.length !== 1) fail(`expected one current-build address row, observed ${unique.length}`);
  const match = /^address:(.*?) ->>>> hash:(\d+) ->>>> bundleHash:(\d+)/.exec(unique[0]);
  if (!match) fail("current-build address row has an unknown encoding");
  const observed = {
    address: match[1],
    address_hash: Number(match[2]),
    bundle_hash: Number(match[3]),
  };
  if (
    observed.address !== TARGET_ADDRESS ||
    observed.address_hash !== TARGET_ADDRESS_HASH ||
    observed.bundle_hash !== TARGET_BUNDLE_HASH
  ) {
    fail("current-build skill logic address identity changed");
  }
  return observed;
}

function readExactEntry(packageFile, entry) {
  const fd = openSync(packageFile, "r");
  const result = Buffer.allocUnsafe(entry.bytes);
  let filled = 0;
  try {
    while (filled < result.length) {
      const count = readSync(fd, result, filled, result.length - filled, entry.offset + filled);
      if (!count) fail(`package m${entry.package_index}.pkg ended early`);
      filled += count;
    }
  } finally {
    closeSync(fd);
  }
  return result;
}

function artifact(file, bytes) {
  return { file, bytes: bytes.length, sha256: sha256(bytes) };
}

function validateProof(proof, bundleBytes) {
  if (
    proof.schema_version !== SCHEMA_VERSION ||
    proof.generated_by !== GENERATED_BY ||
    proof.game !== "blue-protocol-star-resonance" ||
    proof.deployment !== EXPECTED_DEPLOYMENT ||
    proof.channel !== EXPECTED_CHANNEL ||
    proof.build !== EXPECTED_BUILD ||
    proof.address_identity?.address !== TARGET_ADDRESS ||
    Number(proof.address_identity?.address_hash) !== TARGET_ADDRESS_HASH ||
    Number(proof.address_identity?.bundle_hash) !== TARGET_BUNDLE_HASH ||
    proof.bundle?.magic !== "UnityFS" ||
    Number(proof.bundle?.bytes) !== bundleBytes.length ||
    proof.bundle?.sha256 !== sha256(bundleBytes) ||
    proof.authority?.exact_build_skill_logic_carrier_proven !== true ||
    proof.authority?.stage_logic_payload_decoded !== false ||
    proof.authority?.packet_owner_stage_to_stage_type_mapping_proven !== false ||
    proof.authority?.runtime_promotion_allowed !== false ||
    proof.authority?.provider_rdps_credit_allowed !== false
  ) {
    fail("skill logic bundle proof is not fail-closed exact-build evidence");
  }
}

async function analyze(args) {
  if (args.build !== EXPECTED_BUILD) fail(`this reviewed proof supports only build ${EXPECTED_BUILD}`);
  if (existsSync(args.output) || existsSync(args.outputBundle)) fail("refusing to overwrite an existing output");
  const identityBytes = readFileSync(args.identity);
  const identity = JSON.parse(identityBytes);
  const manifestBytes = readFileSync(args.installedManifest);
  const manifest = JSON.parse(manifestBytes);
  if (
    identity.deployment !== EXPECTED_DEPLOYMENT ||
    String(identity.game_build) !== EXPECTED_BUILD ||
    identity.game_assembly?.sha256 !== "4ba9e3f194bfd1769e57e3f12d192208e4d34db04374636738dfc9d5525495a4" ||
    manifest.deployment !== EXPECTED_DEPLOYMENT ||
    manifest.channel !== EXPECTED_CHANNEL ||
    String(manifest.gameBuild) !== EXPECTED_BUILD ||
    manifest.coverage?.complete !== true
  ) {
    fail("identity inputs are not the reviewed complete exact build");
  }
  const exactFiles = [];
  for (const relativePath of EXPECTED_FILES.keys()) {
    exactFiles.push(await verifyExactFile(args.gameRoot, manifest, relativePath));
  }
  const m0 = exactFiles.find((entry) => entry.relative_path.endsWith("/m0.pkg"));
  const meta = exactFiles.find((entry) => entry.relative_path.endsWith("/meta.pkg"));
  const addressIdentity = findCatalogIdentity(m0.file);
  const entries = readMetaEntries(readFileSync(meta.file));
  const matchingEntries = entries.filter((entry) => entry.key === TARGET_BUNDLE_HASH);
  if (matchingEntries.length !== 1) fail(`expected one bundle entry, observed ${matchingEntries.length}`);
  const entry = matchingEntries[0];
  if (entry.type !== 0) fail(`skill logic carrier has unexpected meta.pkg type ${entry.type}`);
  const packageRelativePath = `bpsr/BPSR_STEAM_Data/StreamingAssets/container/m${entry.package_index}.pkg`;
  const packageManifest = manifestEntry(manifest, packageRelativePath);
  const packageFile = relativeToFile(args.gameRoot, packageRelativePath);
  if (!existsSync(packageFile) || statSync(packageFile).size !== Number(packageManifest.bytes)) {
    fail(`installed client package m${entry.package_index}.pkg does not match its exact-build manifest size`);
  }
  const bundleBytes = readExactEntry(packageFile, entry);
  if (bundleBytes.toString("ascii", 0, 7) !== "UnityFS") fail("addressed skill logic carrier is not UnityFS");
  writeFileSync(args.outputBundle, bundleBytes, { flag: "wx" });
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game: "blue-protocol-star-resonance",
    deployment: EXPECTED_DEPLOYMENT,
    channel: EXPECTED_CHANNEL,
    build: EXPECTED_BUILD,
    identity: {
      client_binary_identity: artifact(args.identity, identityBytes),
      installed_client_manifest: artifact(args.installedManifest, manifestBytes),
      exact_files: exactFiles.map(({ file, ...entryValue }) => entryValue),
    },
    address_identity: addressIdentity,
    container_entry: {
      ...entry,
      package_relative_path: packageRelativePath,
      package_manifest_bytes: Number(packageManifest.bytes),
      package_manifest_sha256: String(packageManifest.sha256),
    },
    bundle: {
      ...artifact(args.outputBundle, bundleBytes),
      magic: "UnityFS",
    },
    authority: {
      exact_build_skill_logic_carrier_proven: true,
      stage_logic_payload_decoded: false,
      packet_owner_stage_to_stage_type_mapping_proven: false,
      runtime_promotion_allowed: false,
      provider_rdps_credit_allowed: false,
    },
    next_required_proof: [
      "decode the addressed ZLogicDataSourceTotal payload with the exact MemoryPack member order",
      "join SkillDict key and StageLogicList index/StageId to packet owner_stage without conflating the index with EStageType",
      "replay provider-removed speed opportunities with exact ordering, integer rounding, and party-damage conservation",
    ],
  };
  validateProof(report, bundleBytes);
  writeFileSync(args.output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote exact-build skill logic carrier proof: ${args.output}`);
  console.log(`bundle ${TARGET_BUNDLE_HASH}: m${entry.package_index}.pkg @ ${entry.offset}, ${entry.bytes} bytes, sha256 ${report.bundle.sha256}`);
}

function verify(args) {
  const proof = JSON.parse(readFileSync(args.input, "utf8"));
  const bundleBytes = readFileSync(args.bundle);
  validateProof(proof, bundleBytes);
  console.log(`verified exact-build skill logic carrier: ${args.input}`);
}

const args = parseArgs(process.argv.slice(2));
if (args.command === "analyze") await analyze(args);
else verify(args);
