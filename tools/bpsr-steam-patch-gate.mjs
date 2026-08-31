#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const pluginInventoryRoot = path.join(
  repoRoot,
  "plugins",
  "games",
  "blue-protocol-star-resonance",
  "research",
  "game-file-inventory",
  "global",
);

const sentinels = [
  sentinel("native-code", "bpsr/GameAssembly.dll", "protocol-and-native-schema"),
  sentinel(
    "il2cpp-metadata",
    "bpsr/BPSR_STEAM_Data/il2cpp_data/Metadata/global-metadata.dat",
    "protocol-and-native-schema",
  ),
  sentinel("package-index", "bpsr/files.meta3", "game-data-content"),
  sentinel(
    "unity-bootstrap",
    "bpsr/BPSR_STEAM_Data/globalgamemanagers",
    "unity-runtime-metadata",
  ),
  sentinel(
    "unity-resources",
    "bpsr/BPSR_STEAM_Data/resources.assets",
    "presentation-or-embedded-resources",
  ),
];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "snapshot") await snapshot(options);
else if (command === "diff") diff(options);
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function snapshot(options) {
  const manifestPath = resolvePath(required(options, "appmanifest"));
  const manifest = parseVdf(readFileSync(manifestPath, "utf8"));
  const app = manifest.AppState;
  if (!app || String(app.appid) !== "3681810") {
    throw new Error(`Expected Steam app 3681810 in ${manifestPath}`);
  }

  const steamappsRoot = path.dirname(manifestPath);
  const installRoot = path.join(steamappsRoot, "common", app.installdir);
  const buildId = String(app.buildid);
  const outputPath = resolvePath(
    options.output
      || path.join(pluginInventoryRoot, `steam-${buildId}`, "steam-distribution-snapshot.v1.json"),
  );

  const snapshotValue = {
    schemaVersion: 1,
    generatedBy: "tools/bpsr-steam-patch-gate.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    app: {
      appId: String(app.appid),
      name: String(app.name),
      buildId,
      targetBuildId: String(app.TargetBuildID || app.buildid),
      lastUpdatedUnix: integerOrNull(app.LastUpdated),
      updateResult: String(app.UpdateResult ?? ""),
      stateFlags: String(app.StateFlags ?? ""),
    },
    installedDepots: Object.entries(app.InstalledDepots || {})
      .map(([depotId, value]) => ({
        depotId: String(depotId),
        manifestId: String(value.manifest),
        sizeBytes: integerOrNull(value.size),
      }))
      .sort((left, right) => left.depotId.localeCompare(right.depotId, undefined, { numeric: true })),
    steamDbHint: {
      authority: "routing-only-unofficial-third-party",
      changeNumber: stringOrNull(options.steamdbChangeNumber),
      observedBuildId: stringOrNull(options.steamdbBuildId),
      lastRecordUpdate: stringOrNull(options.steamdbLastRecordUpdate),
      sourceUrl: "https://steamdb.info/app/3681810/",
    },
    authority: {
      steamDb: "early-change-notification-only",
      steamAppManifest: "installed-distribution-identity",
      sentinelHashes: "installed-file-change-routing",
      extractedSemanticRows: "mechanics-and-attribution-proof",
      packetReplay: "runtime-behavior-proof",
    },
    privacy: {
      absoluteInstallPathsStored: false,
      steamOwnerIdStored: false,
      accountDataStored: false,
    },
    sentinels: [],
    routingFingerprintSha256: "",
  };

  for (const definition of sentinels) {
    const filePath = path.join(installRoot, ...definition.relativePath.split("/"));
    const present = existsSync(filePath);
    const bytes = present ? readFileSize(filePath) : null;
    snapshotValue.sentinels.push({
      id: definition.id,
      relativePath: definition.relativePath,
      routesTo: definition.routesTo,
      present,
      usable: present && bytes > 0,
      bytes,
      sha256: present ? await hashFile(filePath) : null,
    });
  }

  snapshotValue.routingFingerprintSha256 = hash(canonical({
    buildId: snapshotValue.app.buildId,
    targetBuildId: snapshotValue.app.targetBuildId,
    installedDepots: snapshotValue.installedDepots,
    sentinels: snapshotValue.sentinels,
  }));

  writeJson(outputPath, snapshotValue);
  console.log(`Steam build ${buildId}; depot manifests: ${snapshotValue.installedDepots.length}`);
  console.log(`Hashed ${snapshotValue.sentinels.filter((item) => item.present).length}/${snapshotValue.sentinels.length} routing sentinels; ${snapshotValue.sentinels.filter((item) => item.usable).length} are non-empty.`);
  console.log(`Wrote ${relativeRepo(outputPath)}`);
}

function diff(options) {
  const baselinePath = resolvePath(required(options, "baseline"));
  const candidatePath = resolvePath(required(options, "candidate"));
  const baseline = readJson(baselinePath);
  const candidate = readJson(candidatePath);
  validateSnapshot(baseline, "baseline");
  validateSnapshot(candidate, "candidate");

  const baselineDepots = new Map((baseline.installedDepots || []).map((item) => [item.depotId, item]));
  const candidateDepots = new Map((candidate.installedDepots || []).map((item) => [item.depotId, item]));
  const depotChanges = [...new Set([...baselineDepots.keys(), ...candidateDepots.keys()])]
    .sort((left, right) => left.localeCompare(right, undefined, { numeric: true }))
    .filter((depotId) => canonical(baselineDepots.get(depotId)) !== canonical(candidateDepots.get(depotId)))
    .map((depotId) => ({ depotId, baseline: baselineDepots.get(depotId) || null, candidate: candidateDepots.get(depotId) || null }));

  const baselineSentinels = new Map((baseline.sentinels || []).map((item) => [item.id, item]));
  const candidateSentinels = new Map((candidate.sentinels || []).map((item) => [item.id, item]));
  const sentinelChanges = [...new Set([...baselineSentinels.keys(), ...candidateSentinels.keys()])]
    .sort()
    .filter((id) => canonical(baselineSentinels.get(id)) !== canonical(candidateSentinels.get(id)))
    .map((id) => ({
      id,
      routesTo: candidateSentinels.get(id)?.routesTo || baselineSentinels.get(id)?.routesTo || "unknown",
      baselineSha256: baselineSentinels.get(id)?.sha256 || null,
      candidateSha256: candidateSentinels.get(id)?.sha256 || null,
    }));

  const routes = [...new Set(sentinelChanges.map((item) => item.routesTo))].sort();
  if (depotChanges.length && !routes.length) routes.push("depot-other-files");
  const buildChanged = baseline.app.buildId !== candidate.app.buildId;
  const noInstalledChange = !buildChanged && !depotChanges.length && !sentinelChanges.length;
  const metadataOnlyBuildChange = buildChanged && !depotChanges.length && !sentinelChanges.length;
  const actions = routeActions(routes, { noInstalledChange, metadataOnlyBuildChange });
  const output = {
    schemaVersion: 1,
    generatedBy: "tools/bpsr-steam-patch-gate.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    baselineBuildId: baseline.app.buildId,
    candidateBuildId: candidate.app.buildId,
    buildChanged,
    depotChanges,
    sentinelChanges,
    routes,
    verdict: noInstalledChange
      ? "no-installed-change-skip-extraction"
      : metadataOnlyBuildChange
        ? "steam-build-metadata-only-verify-before-extraction"
        : "installed-change-run-routed-extraction",
    requiredActions: actions,
    policy: {
      steamDbNeverMechanicsAuthority: true,
      depotManifestNeverMechanicsAuthority: true,
      noRuntimeRuleAutoPromotion: true,
      semanticRowDiffRequiredAfterExtraction: true,
      packetReplayRequiredForBehaviorChanges: true,
    },
  };
  const outputPath = options.output ? resolvePath(options.output) : null;
  if (outputPath) writeJson(outputPath, output);
  console.log(`${output.baselineBuildId} -> ${output.candidateBuildId}: ${output.verdict}`);
  console.log(`Depot changes: ${depotChanges.length}; sentinel changes: ${sentinelChanges.length}`);
  console.log(`Routes: ${routes.join(", ") || "none"}`);
  for (const action of actions) console.log(`- ${action}`);
}

function routeActions(routes, state) {
  if (state.noInstalledChange) return ["skip-all-extraction-and-proof-regeneration"];
  if (state.metadataOnlyBuildChange) {
    return ["record-the-new-distribution-build", "verify-package-index-before-reusing-the-prior-semantic-manifest"];
  }
  const actions = new Set();
  if (routes.includes("protocol-and-native-schema")) {
    actions.add("rerun-il2cpp-and-protobuf-schema-extraction");
    actions.add("audit-versioned-protocol-pack-and-decoder-migration");
  }
  if (routes.includes("game-data-content") || routes.includes("depot-other-files")) {
    actions.add("extract-game-tables-once");
    actions.add("run-seasonal-domain-semantic-diff");
    actions.add("regenerate-only-changed-domain-derived-ledgers");
  }
  if (routes.includes("unity-runtime-metadata")) actions.add("audit-unity-runtime-and-scene-bootstrap-metadata");
  if (routes.includes("presentation-or-embedded-resources")) actions.add("refresh-localization-and-icon-reference-assets");
  actions.add("do-not-reset-proven-rdps-rules-whose-exact-inputs-are-unchanged");
  return [...actions];
}

function sentinel(id, relativePath, routesTo) {
  return { id, relativePath, routesTo };
}

function parseVdf(text) {
  const tokens = [];
  const matcher = /"((?:\\.|[^"\\])*)"|([{}])/g;
  let match;
  while ((match = matcher.exec(text)) !== null) {
    tokens.push(match[2] || match[1].replace(/\\\\/g, "\\").replace(/\\"/g, "\""));
  }
  let index = 0;
  const root = {};
  while (index < tokens.length) {
    const key = tokens[index++];
    if (tokens[index] === "{") {
      index += 1;
      root[key] = readObject();
    } else root[key] = tokens[index++];
  }
  return root;

  function readObject() {
    const output = {};
    while (index < tokens.length && tokens[index] !== "}") {
      const key = tokens[index++];
      if (tokens[index] === "{") {
        index += 1;
        output[key] = readObject();
      } else output[key] = tokens[index++];
    }
    if (tokens[index] !== "}") throw new Error("Unterminated VDF object");
    index += 1;
    return output;
  }
}

async function hashFile(filePath) {
  const digest = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) digest.update(chunk);
  return digest.digest("hex");
}

function readFileSize(filePath) {
  return statSync(filePath).size;
}

function validateSnapshot(value, label) {
  if (value?.generatedBy !== "tools/bpsr-steam-patch-gate.mjs" || !value?.app?.buildId) {
    throw new Error(`${label} is not a BPSR Steam patch-gate snapshot`);
  }
}

function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`);
    const key = arg.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    const next = args[index + 1];
    if (!next || next.startsWith("--")) output[key] = true;
    else {
      output[key] = next;
      index += 1;
    }
  }
  return output;
}

function required(options, key) {
  if (!options[key]) throw new Error(`Missing --${key.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`);
  return options[key];
}

function integerOrNull(value) {
  const parsed = Number.parseInt(String(value), 10);
  return Number.isFinite(parsed) ? parsed : null;
}

function stringOrNull(value) {
  return value === undefined || value === true || value === "" ? null : String(value);
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relativeRepo(value) {
  return path.relative(repoRoot, value).replaceAll("\\", "/");
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function writeJson(filePath, value) {
  mkdirSync(path.dirname(filePath), { recursive: true });
  writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

function canonical(value) {
  if (value === undefined) return "undefined";
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function hash(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function selfTest() {
  const parsed = parseVdf(`"AppState" { "appid" "3681810" "InstalledDepots" { "3681812" { "manifest" "9" "size" "10" } } }`);
  if (parsed.AppState.appid !== "3681810") throw new Error("VDF scalar parse failed");
  if (parsed.AppState.InstalledDepots["3681812"].manifest !== "9") throw new Error("VDF nested parse failed");
  const actions = routeActions(["protocol-and-native-schema", "game-data-content"], {});
  if (!actions.includes("rerun-il2cpp-and-protobuf-schema-extraction")) throw new Error("Protocol route missing");
  if (!actions.includes("run-seasonal-domain-semantic-diff")) throw new Error("Content route missing");
  console.log("bpsr-steam-patch-gate self-test passed");
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-steam-patch-gate.mjs snapshot --appmanifest <appmanifest_3681810.acf> [--output <json>] [--steamdb-change-number <id>] [--steamdb-build-id <id>] [--steamdb-last-record-update <date>]
  node tools/bpsr-steam-patch-gate.mjs diff --baseline <json> --candidate <json> [--output <json>]
  node tools/bpsr-steam-patch-gate.mjs self-test`);
  process.exit(exitCode);
}
