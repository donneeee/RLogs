#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  createReadStream,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
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

const routes = [
  route("combat-skills-actions", /(skill|action|combat|bullet|hit|damage|fight|counter|behit|elemental|recount|revive|luckystrike)/i,
    ["combat-table-diff", "origin-graph-diff", "formula-stage-replay"]),
  route("ai-behavior-navigation", /(aitable|botai|bdtag|dbm|behavior|navigation)/i,
    ["game-file-schema-diff", "entity-ownership-replay"]),
  route("combat-buffs-effects-formulas", /(buff|effect|modifier|formula|attribute|attr|prob|coefficient|scalar|mitigation|shield|heal|state)/i,
    ["combat-table-diff", "status-lifecycle-replay", "provider-recipient-replay"]),
  route("talents-seasonal-psychoscope", /(talent|rogue|factor|psychoscope|phantom|season)/i,
    ["origin-graph-diff", "factor-event-correlation", "provider-recipient-replay"]),
  route("equipment-items-weapons", /(equip|weapon|item|affix|suit|recast|refine|enchant|breakthrough|drop|award|gasha|currency|compose|modhole|modinitialization|modtable)/i,
    ["equipment-source-diff", "origin-graph-diff", "status-lifecycle-replay"]),
  route("imagines-pets-summons", /(imagine|aoyi|pet|summon|fantasy)/i,
    ["origin-graph-diff", "provider-recipient-replay", "canonical-replay-conservation"]),
  route("scenes-dungeons-raids-world-entities", /(scene|dungeon|raid|monster|npc|entity|world|zone|field|map|dummy|boss|stage|area|camp|environment|instance|challenge|settlement|dayandnight|terrain|windtunnel|target|robot|stiff|jump|transfer|trap|vehicle)/i,
    ["scene-identity-diff", "encounter-boundary-replay", "entity-ownership-replay"]),
  route("character-profile-progression", /(player|character|avatar|level|achievement|profile|role|appearance|body|profession|class|specialization|growth|progress|modelhuman|personal|mentor|medal|bless|lifeexp|assess|planetmemory)/i,
    ["profile-schema-diff", "party-identity-replay"]),
  route("localization-presentation-assets", /(name|description|local|language|icon|texture|picture|photo|background|effectlibrary|animation|anim|audio|music|voice|subtitle|text|color|font|ui|guide|help|emoclip|footstep|presentation|model)/i,
    ["presentation-reference-diff", "localization-pack-rebuild"]),
  route("social-guild-chat", /(guild|union|team|party|social|friend|chat|channel|contact|mail|ban|follow|message)/i,
    ["social-schema-diff", "party-identity-replay"]),
  route("economy-crafting-life", /(craft|cook|cuisine|chemistry|fishing|recipe|material|exchange|market|shop|trade|hobby|collection|energy|mall|payment|recharge|stall|recycle|obtainway|monthcard|life)/i,
    ["game-system-schema-diff"]),
  route("housing-fashion-photo", /(home|house|housing|residential|fashion|face|facial|emote|sticker|dance|photo|headwear|toy|seat)/i,
    ["game-system-schema-diff", "presentation-reference-diff"]),
  route("quests-story-cinematics", /(quest|story|chapter|episode|dialogue|cutscene|cg|task|explore|investigation|clue|plot|note|scenicspot)/i,
    ["game-system-schema-diff", "scene-identity-diff"]),
  route("activities-minigames", /(activity|battlepass|casual|band|musical|climb|bubble|performance|signin|activation|mahjong|leaderboard|match|qte|parkour|trialroad|parade|tdbook|tdtower|weeklytreasure)/i,
    ["game-system-schema-diff", "scene-identity-diff"]),
  route("system-client-ui-config", /(config|system|function|condition|rule|service|gm|report|queue|search|preview|limit|loading|setting|keyboard|privilege|reddot|interact|ignore|resolve|inference|sample|pivot|visual|voxel)/i,
    ["game-file-schema-diff"]),
];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "scan") await scan(options);
else if (command === "diff") diff(options);
else if (command === "verify") await verify(options);
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function scan(options) {
  const build = required(options, "build");
  const roots = resolveRoots(build, options);
  const output = resolvePath(options.output || path.join(
    pluginInventoryRoot,
    `steam-${build}`,
    "complete-build-source-manifest.v1.json",
  ));
  const steamSnapshotPath = resolvePath(options.steamSnapshot || path.join(
    pluginInventoryRoot,
    `steam-${build}`,
    "steam-distribution-snapshot.v1.json",
  ));
  const manifest = await buildManifest(build, roots, steamSnapshotPath);
  writeJson(output, manifest);
  reportScan(manifest, output);
  if (!manifest.coverage.complete || manifest.missingRoots.length) process.exitCode = 2;
}

async function buildManifest(build, roots, steamSnapshotPath) {
  const manifest = {
    schemaVersion: 1,
    generatedBy: "tools/bpsr-build-source-manifest.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    gameBuild: String(build),
    distribution: readDistributionIdentity(steamSnapshotPath, build),
    authority: {
      steamDb: "patch-alarm-and-routing-only",
      steamAppManifest: "installed-distribution-identity",
      decodedGameTables: "exact-current-build-static-data",
      generatedResearchFiles: "derived-indexes-requiring-regeneration",
      packetReplay: "runtime-behavior-proof",
    },
    policy: {
      storesAbsolutePaths: false,
      storesAccountOrOwnerData: false,
      excludesUnknownFiles: false,
      routeClassificationIsMechanicsTruth: false,
      fallbackRoute: "other-game-systems",
      unknownFilesRemainVisible: true,
      semanticPromotionRequiresPacketReplay: true,
    },
    roots: [],
    files: [],
    missingRoots: [],
    missingRequiredFiles: [],
    routeSummary: {},
    relatedRouteSummary: {},
    extensionSummary: {},
    coverage: {
      filesDiscovered: 0,
      filesHashed: 0,
      bytesHashed: 0,
      complete: false,
      silentOmissions: 0,
    },
    aggregateSha256: "",
  };

  for (const root of roots) {
    if (!existsSync(root.path)) {
      manifest.missingRoots.push(root.id);
      manifest.roots.push({ id: root.id, authority: root.authority, present: false, fileCount: 0, bytes: 0 });
      continue;
    }
    const discovered = discoverFiles(root.path);
    for (const relativePath of root.requiredFiles || []) {
      if (!existsSync(path.join(root.path, relativePath))) {
        manifest.missingRequiredFiles.push(`${root.id}:${normalizePath(relativePath)}`);
      }
    }
    const rootRecord = { id: root.id, authority: root.authority, present: true, fileCount: discovered.length, bytes: 0 };
    manifest.coverage.filesDiscovered += discovered.length;
    for (const absolutePath of discovered) {
      const relativePath = normalizePath(path.relative(root.path, absolutePath));
      const stat = lstatSync(absolutePath);
      if (!stat.isFile()) continue;
      const classification = classify(`${relativePath} ${path.basename(relativePath, path.extname(relativePath))}`);
      const record = {
        id: `${root.id}:${relativePath}`,
        root: root.id,
        authority: root.authority,
        relativePath,
        extension: path.extname(relativePath).toLowerCase() || "<none>",
        bytes: stat.size,
        sha256: await hashFile(absolutePath),
        route: classification.id,
        relatedRoutes: classification.relatedRoutes,
        routeConfidence: classification.confidence,
        routeReason: classification.reason,
        proofSuites: classification.proofSuites,
      };
      manifest.files.push(record);
      rootRecord.bytes += stat.size;
      manifest.coverage.filesHashed += 1;
      manifest.coverage.bytesHashed += stat.size;
      manifest.routeSummary[record.route] = (manifest.routeSummary[record.route] || 0) + 1;
      for (const routeId of record.relatedRoutes) {
        manifest.relatedRouteSummary[routeId] = (manifest.relatedRouteSummary[routeId] || 0) + 1;
      }
      manifest.extensionSummary[record.extension] = (manifest.extensionSummary[record.extension] || 0) + 1;
    }
    manifest.roots.push(rootRecord);
  }

  manifest.files.sort((a, b) => a.id.localeCompare(b.id));
  manifest.roots.sort((a, b) => a.id.localeCompare(b.id));
  manifest.routeSummary = sortedObject(manifest.routeSummary);
  manifest.relatedRouteSummary = sortedObject(manifest.relatedRouteSummary);
  manifest.extensionSummary = sortedObject(manifest.extensionSummary);
  manifest.coverage.silentOmissions = manifest.coverage.filesDiscovered - manifest.coverage.filesHashed;
  manifest.missingRequiredFiles.sort();
  manifest.coverage.complete = manifest.missingRoots.length === 0
    && manifest.missingRequiredFiles.length === 0
    && manifest.coverage.silentOmissions === 0
    && manifest.coverage.filesDiscovered === manifest.files.length;
  manifest.aggregateSha256 = hashText(manifest.files
    .map((file) => `${file.id}:${file.bytes}:${file.sha256}:${file.route}`)
    .join("\n"));
  return manifest;
}

function diff(options) {
  const baselinePath = resolveManifestPath(options, "baseline", "baselineBuild");
  const candidatePath = resolveManifestPath(options, "candidate", "candidateBuild");
  const baseline = readJson(baselinePath);
  const candidate = readJson(candidatePath);
  const before = new Map((baseline.files || []).map((file) => [file.id, file]));
  const after = new Map((candidate.files || []).map((file) => [file.id, file]));
  const addedFiles = [...after.keys()].filter((id) => !before.has(id)).sort();
  const removedFiles = [...before.keys()].filter((id) => !after.has(id)).sort();
  const changedFiles = [...after.keys()].filter((id) => before.has(id)
    && (before.get(id).sha256 !== after.get(id).sha256 || before.get(id).bytes !== after.get(id).bytes)).sort();
  const routeChanges = {};
  for (const [kind, ids, source] of [
    ["added", addedFiles, after],
    ["removed", removedFiles, before],
    ["changed", changedFiles, after],
  ]) {
    for (const id of ids) {
      const file = source.get(id);
      for (const routeId of file.relatedRoutes || [file.route]) {
        const bucket = routeChanges[routeId] ||= { added: [], removed: [], changed: [], proofSuites: new Set() };
        bucket[kind].push(id);
        for (const suite of file.proofSuites || []) bucket.proofSuites.add(suite);
      }
    }
  }
  const changedRoutes = Object.entries(routeChanges).sort(([a], [b]) => a.localeCompare(b)).map(([routeId, value]) => ({
    route: routeId,
    addedFiles: value.added,
    removedFiles: value.removed,
    changedFiles: value.changed,
    proofSuites: [...value.proofSuites].sort(),
  }));
  const output = {
    schemaVersion: 1,
    generatedBy: "tools/bpsr-build-source-manifest.mjs",
    baselineBuild: baseline.gameBuild,
    candidateBuild: candidate.gameBuild,
    baselineDistribution: baseline.distribution,
    candidateDistribution: candidate.distribution,
    aggregateChanged: baseline.aggregateSha256 !== candidate.aggregateSha256,
    addedFiles,
    removedFiles,
    changedFiles,
    changedRoutes,
    requiredRescans: changedRoutes.map((entry) => ({ route: entry.route, proofSuites: entry.proofSuites })),
    coverage: {
      baselineComplete: baseline.coverage?.complete === true,
      candidateComplete: candidate.coverage?.complete === true,
    },
  };
  const outputPath = resolvePath(options.output || path.join(
    path.dirname(candidatePath),
    `complete-build-source-diff-from-${baseline.gameBuild}.v1.json`,
  ));
  writeJson(outputPath, output);
  console.log(`Changed files: ${changedFiles.length}; added: ${addedFiles.length}; removed: ${removedFiles.length}`);
  console.log(`Changed routes: ${changedRoutes.length}`);
  console.log(`Wrote ${relativeRepo(outputPath)}`);
}

async function verify(options) {
  const manifestPath = resolvePath(required(options, "manifest"));
  const expected = readJson(manifestPath);
  const roots = resolveRoots(expected.gameBuild, options);
  const actual = await buildManifest(expected.gameBuild, roots, "");
  const expectedFiles = new Map((expected.files || []).map((file) => [file.id, file]));
  const actualFiles = new Map((actual.files || []).map((file) => [file.id, file]));
  const missing = [...expectedFiles.keys()].filter((id) => !actualFiles.has(id)).sort();
  const unexpected = [...actualFiles.keys()].filter((id) => !expectedFiles.has(id)).sort();
  const changed = [...actualFiles.keys()].filter((id) => expectedFiles.has(id)
    && actualFiles.get(id).sha256 !== expectedFiles.get(id).sha256).sort();
  const result = { complete: actual.coverage.complete, missing, unexpected, changed };
  console.log(JSON.stringify(result, null, 2));
  if (!result.complete || missing.length || unexpected.length || changed.length) process.exitCode = 2;
}

async function selfTest() {
  const temporaryRoot = mkdtempSync(path.join(os.tmpdir(), "rlogs-build-manifest-"));
  try {
    const decoded = path.join(temporaryRoot, "decoded");
    const extracted = path.join(temporaryRoot, "extracted");
    mkdirSync(decoded, { recursive: true });
    mkdirSync(extracted, { recursive: true });
    writeFileSync(path.join(decoded, "SkillTable.json"), "{\"1\":{\"Id\":1}}\n");
    writeFileSync(path.join(decoded, "UnknownTable.json"), "[]\n");
    writeFileSync(path.join(extracted, "BuffDescriptions.json"), "{}\n");
    writeFileSync(path.join(extracted, "NOTES.md"), "proof\n");
    const manifest = await buildManifest("test", [
      { id: "decoded-game-tables", authority: "exact-current-build-static-data", path: decoded },
      { id: "generated-research", authority: "derived-current-build-index", path: extracted },
    ], "");
    assert(manifest.coverage.complete, "coverage must be complete");
    assert(manifest.files.length === 4, "all files must be retained");
    assert(manifest.routeSummary["other-game-systems"] >= 1, "fallback-routed files must remain visible");
    assert(!JSON.stringify(manifest).includes(temporaryRoot), "absolute paths must not be stored");
    console.log("bpsr-build-source-manifest self-test passed");
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

function resolveRoots(build, options) {
  const exactDefault = path.join("..", "BPSR-UID-Extractors", `output-build-${build}-exact`);
  const legacyDefault = path.join("..", "BPSR-UID-Extractors", `output-build-${build}`);
  const extractorDefault = existsSync(resolvePath(exactDefault)) ? exactDefault : legacyDefault;
  return [
    {
      id: "decoded-game-tables",
      authority: "exact-current-build-static-data",
      path: resolvePath(options.decodedRoot || path.join("..", ".codex_tmp", `current-build-${build}-table-extract-candidate`, "Excels")),
    },
    {
      id: "generated-research",
      authority: "derived-current-build-index",
      path: resolvePath(options.extractorRoot || extractorDefault),
      requiredFiles: ["BattleImagineBehaviorLinks.json"],
    },
  ];
}

function classify(value) {
  const matches = routes.filter((candidate) => candidate.pattern.test(value));
  if (!matches.length) return {
    id: "other-game-systems",
    relatedRoutes: ["other-game-systems"],
    confidence: "explicit-fallback-route",
    reason: "No specialist routing keyword matched; retained in the explicit other-game-systems route for byte-level patch diff and manual semantic review.",
    proofSuites: ["manual-domain-classification", "game-file-schema-diff"],
  };
  const selected = matches[0];
  return {
    id: selected.id,
    relatedRoutes: matches.map((match) => match.id),
    confidence: matches.length === 1 ? "keyword-exact-route" : "keyword-priority-route",
    reason: matches.length === 1
      ? `Filename matched the ${selected.id} routing vocabulary.`
      : `Filename matched multiple routing vocabularies; ${selected.id} won deterministic priority.`,
    proofSuites: [...new Set(matches.flatMap((match) => match.proofSuites))].sort(),
  };
}

function route(id, pattern, proofSuites) { return { id, pattern, proofSuites }; }
function discoverFiles(root) {
  const output = [];
  const pending = [root];
  while (pending.length) {
    const current = pending.pop();
    const entries = readdirSync(current, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name));
    for (const entry of entries) {
      const target = path.join(current, entry.name);
      if (entry.isDirectory()) pending.push(target);
      else if (entry.isFile()) output.push(target);
    }
  }
  return output.sort((a, b) => a.localeCompare(b));
}
function hashFile(file) {
  return new Promise((resolve, reject) => {
    const digest = createHash("sha256");
    const input = createReadStream(file);
    input.on("data", (chunk) => digest.update(chunk));
    input.on("error", reject);
    input.on("end", () => resolve(digest.digest("hex")));
  });
}
function readDistributionIdentity(snapshotPath, build) {
  if (!snapshotPath || !existsSync(snapshotPath)) return { buildId: String(build), snapshotPresent: false };
  const snapshot = readJson(snapshotPath);
  return {
    buildId: String(snapshot.app?.buildId || build),
    appId: snapshot.app?.appId || null,
    depots: (snapshot.installedDepots || []).map((depot) => ({
      depotId: depot.depotId,
      manifestId: depot.manifestId,
      sizeBytes: depot.sizeBytes,
    })),
    steamDbChangeNumber: snapshot.steamDbHint?.changeNumber || null,
    steamDbLastRecordUpdate: snapshot.steamDbHint?.lastRecordUpdate || null,
    routingFingerprintSha256: snapshot.routingFingerprintSha256 || null,
    snapshotPresent: true,
  };
}
function resolveManifestPath(options, fileKey, buildKey) {
  if (options[fileKey]) return resolvePath(options[fileKey]);
  const build = required(options, buildKey);
  return path.join(pluginInventoryRoot, `steam-${build}`, "complete-build-source-manifest.v1.json");
}
function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument: ${token}`);
    const key = token.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    const next = args[index + 1];
    if (!next || next.startsWith("--")) result[key] = true;
    else { result[key] = next; index += 1; }
  }
  return result;
}
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`); return value[key]; }
function resolvePath(value) { return path.resolve(repoRoot, value); }
function relativeRepo(value) { return normalizePath(path.relative(repoRoot, value)); }
function normalizePath(value) { return value.split(path.sep).join("/"); }
function readJson(file) { return JSON.parse(readFileSync(file, "utf8")); }
function writeJson(file, value) { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function sortedObject(value) { return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b))); }
function assert(condition, message) { if (!condition) throw new Error(`Self-test failed: ${message}`); }
function reportScan(manifest, output) {
  console.log(`Hashed ${manifest.coverage.filesHashed}/${manifest.coverage.filesDiscovered} files (${manifest.coverage.bytesHashed} bytes).`);
  console.log(`Coverage complete: ${manifest.coverage.complete}; silent omissions: ${manifest.coverage.silentOmissions}.`);
  console.log(`Fallback-routed but retained: ${manifest.routeSummary["other-game-systems"] || 0}.`);
  console.log(`Wrote ${relativeRepo(output)}`);
}
function usage(exitCode) {
  console.log("Usage:");
  console.log("  node tools/bpsr-build-source-manifest.mjs scan --build <id> [--extractor-root <dir>] [--decoded-root <dir>]");
  console.log("  node tools/bpsr-build-source-manifest.mjs diff --baseline-build <id> --candidate-build <id>");
  console.log("  node tools/bpsr-build-source-manifest.mjs verify --manifest <file> [--extractor-root <dir>] [--decoded-root <dir>]");
  console.log("  node tools/bpsr-build-source-manifest.mjs self-test");
  process.exit(exitCode);
}
