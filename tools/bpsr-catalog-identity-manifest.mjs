import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-catalog-identity-manifest.mjs";

function fail(message) {
  throw new Error(message);
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) fail(`invalid option near ${key ?? "<end>"}`);
    options[key.slice(2)] = value;
  }
  return options;
}

function required(options, key) {
  const value = options[key];
  if (!value) fail(`missing --${key}`);
  return value;
}

function normalizedRelative(root, file) {
  const relative = path.relative(root, file).replaceAll("\\", "/");
  if (!relative || relative === ".." || relative.startsWith("../") || path.isAbsolute(relative)) {
    fail(`path escapes catalog root: ${file}`);
  }
  return relative;
}

function collectFiles(root, directory = root, output = []) {
  const entries = readdirSync(directory, { withFileTypes: true })
    .sort((left, right) => left.name.localeCompare(right.name, "en"));
  for (const entry of entries) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) collectFiles(root, file, output);
    else if (entry.isFile()) output.push({ file, relative: normalizedRelative(root, file) });
  }
  return output;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function buildMatches(identity, gameBuild) {
  const value = String(identity);
  return value === gameBuild || value.endsWith(`-${gameBuild}`);
}

function rootBuildIdentities(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return [];
  const identities = [];
  for (const key of ["game_build", "client_build", "build_id"]) {
    if (typeof value[key] === "string" && value[key].trim()) identities.push(value[key].trim());
  }
  if (Array.isArray(value.supported_builds)) {
    for (const entry of value.supported_builds) {
      if (!entry || typeof entry !== "object") continue;
      for (const key of ["game_build", "client_build", "build_id"]) {
        if (typeof entry[key] === "string" && entry[key].trim()) identities.push(entry[key].trim());
      }
    }
  }
  return [...new Set(identities)].sort();
}

function classifyFile(relative, bytes, gameBuild) {
  const base = {
    path: relative,
    bytes: bytes.length,
    sha256: sha256(bytes),
  };
  if (!relative.toLowerCase().endsWith(".json")) {
    return { ...base, kind: "non-json", declared_builds: [], identity_state: "unversioned" };
  }
  let value;
  try {
    value = JSON.parse(bytes.toString("utf8"));
  } catch {
    return { ...base, kind: "json", declared_builds: [], identity_state: "invalid-json" };
  }
  const declaredBuilds = rootBuildIdentities(value);
  const matching = declaredBuilds.filter((identity) => buildMatches(identity, gameBuild));
  let identityState = "unversioned";
  if (matching.length === declaredBuilds.length && matching.length > 0) identityState = "current-build";
  else if (matching.length > 0) identityState = "mixed-build";
  else if (declaredBuilds.length > 0) identityState = "historical-build";
  return {
    ...base,
    kind: "json",
    declared_builds: declaredBuilds,
    identity_state: identityState,
  };
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function contentSha256(value) {
  return sha256(Buffer.from(JSON.stringify(stable(value)), "utf8"));
}

function treeSha256(entries) {
  const hash = createHash("sha256");
  for (const entry of entries) {
    const relative = Buffer.from(entry.path, "utf8");
    const digest = Buffer.from(entry.sha256, "hex");
    hash.update(Buffer.from(BigInt(relative.length).toString(16).padStart(16, "0"), "hex"));
    hash.update(relative);
    hash.update(Buffer.from(BigInt(entry.bytes).toString(16).padStart(16, "0"), "hex"));
    hash.update(digest);
  }
  return hash.digest("hex");
}

function buildManifest({ root, output, gameBuild, deployment, channel }) {
  const rootPath = path.resolve(root);
  const outputPath = path.resolve(output);
  const outputRelative = normalizedRelative(rootPath, outputPath);
  const entries = collectFiles(rootPath)
    .filter((entry) => entry.relative !== outputRelative)
    .map((entry) => classifyFile(entry.relative, readFileSync(entry.file), gameBuild));
  const states = {};
  for (const entry of entries) states[entry.identity_state] = (states[entry.identity_state] ?? 0) + 1;
  const manifest = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    deployment_id: deployment,
    channel,
    catalog_root: path.relative(process.cwd(), rootPath).replaceAll("\\", "/"),
    identity_manifest_relative_path: outputRelative,
    policy: {
      every_source_file_is_hashed: true,
      identity_manifest_is_the_only_snapshot_build_router: true,
      historical_entries_are_retained: true,
      historical_entries_have_runtime_authority: false,
      unversioned_entries_have_runtime_authority: false,
      current_build_entries_are_candidate_evidence_only: true,
      exact_numeric_ids_remain_authoritative: true,
      unresolved_entries_are_hidden: false,
      runtime_promotion_allowed: false,
    },
    summary: {
      indexed_files: entries.length,
      indexed_bytes: entries.reduce((sum, entry) => sum + entry.bytes, 0),
      current_build_entry_count: states["current-build"] ?? 0,
      historical_build_entry_count: states["historical-build"] ?? 0,
      mixed_build_entry_count: states["mixed-build"] ?? 0,
      unversioned_entry_count: states.unversioned ?? 0,
      invalid_json_entry_count: states["invalid-json"] ?? 0,
      identity_state_counts: Object.fromEntries(Object.entries(states).sort()),
    },
    source_tree_sha256: treeSha256(entries),
    entries,
  };
  return { ...manifest, content_sha256: contentSha256(manifest) };
}

function validateManifest(value) {
  assert.equal(value.schema_version, SCHEMA_VERSION);
  assert.equal(value.generated_by, GENERATED_BY);
  assert.equal(typeof value.game_build, "string");
  assert.equal(typeof value.deployment_id, "string");
  assert.equal(typeof value.channel, "string");
  assert.equal(value.policy.every_source_file_is_hashed, true);
  assert.equal(value.policy.identity_manifest_is_the_only_snapshot_build_router, true);
  assert.equal(value.policy.historical_entries_are_retained, true);
  assert.equal(value.policy.historical_entries_have_runtime_authority, false);
  assert.equal(value.policy.unversioned_entries_have_runtime_authority, false);
  assert.equal(value.policy.current_build_entries_are_candidate_evidence_only, true);
  assert.equal(value.policy.exact_numeric_ids_remain_authoritative, true);
  assert.equal(value.policy.unresolved_entries_are_hidden, false);
  assert.equal(value.policy.runtime_promotion_allowed, false);
  assert.ok(Array.isArray(value.entries));
  const withoutHash = structuredClone(value);
  delete withoutHash.content_sha256;
  assert.equal(value.content_sha256, contentSha256(withoutHash));
}

function generate(options) {
  const output = path.resolve(required(options, "output"));
  const manifest = buildManifest({
    root: required(options, "root"),
    output,
    gameBuild: required(options, "build"),
    deployment: required(options, "deployment"),
    channel: required(options, "channel"),
  });
  validateManifest(manifest);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const input = path.resolve(required(options, "input"));
  const value = JSON.parse(readFileSync(input, "utf8"));
  validateManifest(value);
  const root = path.resolve(path.dirname(input), path.relative(path.dirname(input), path.resolve(value.catalog_root)));
  const rebuilt = buildManifest({
    root,
    output: input,
    gameBuild: value.game_build,
    deployment: value.deployment_id,
    channel: value.channel,
  });
  assert.deepEqual(value, rebuilt);
  console.log(input);
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-catalog-manifest-"));
  try {
    writeFileSync(path.join(root, "current.json"), '{"game_build":"2","effect_id":20}\n');
    writeFileSync(path.join(root, "historical.json"), '{"game_build":"1","effect_id":10}\n');
    writeFileSync(path.join(root, "notes.md"), "retained\n");
    const output = path.join(root, "current-build-manifest.v1.json");
    const manifest = buildManifest({
      root,
      output,
      gameBuild: "2",
      deployment: "global",
      channel: "steam",
    });
    validateManifest(manifest);
    assert.deepEqual(manifest.summary.identity_state_counts, {
      "current-build": 1,
      "historical-build": 1,
      unversioned: 1,
    });
    assert.equal(manifest.entries.some((entry) => entry.path === "current-build-manifest.v1.json"), false);
  } finally {
    const resolved = path.resolve(root);
    assert.ok(resolved.startsWith(path.resolve(tmpdir())));
    rmSync(resolved, { recursive: true, force: true });
  }
  console.log("ok");
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else if (command === "self-test") selfTest();
else {
  console.log("Usage:\n  node tools/bpsr-catalog-identity-manifest.mjs generate --root <catalog-dir> --output <manifest.json> --build <id> --deployment <id> --channel <id>\n  node tools/bpsr-catalog-identity-manifest.mjs verify --input <manifest.json>\n  node tools/bpsr-catalog-identity-manifest.mjs self-test");
  process.exit(1);
}
