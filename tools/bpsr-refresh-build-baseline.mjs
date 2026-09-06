#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, readFileSync, unlinkSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "refresh") refresh(options);
else usage(command === "help" ? 0 : 1);

function refresh(options) {
  const build = required(options, "build");
  const inventoryRoot = path.join(
    repoRoot,
    "plugins",
    "games",
    "blue-protocol-star-resonance",
    "research",
    "game-file-inventory",
    "global",
  );
  const buildRoot = path.join(inventoryRoot, `steam-${build}`);
  const distribution = path.join(buildRoot, "steam-distribution-snapshot.v1.json");
  const installedClientManifest = path.join(buildRoot, "installed-client-file-manifest.v1.json");
  const completeManifest = path.join(buildRoot, "complete-build-source-manifest.v1.json");
  const extractorRoot = resolvePath(options.extractorRoot
    || path.join("..", "BPSR-UID-Extractors", `output-build-${build}-exact`));
  const extractorCodeRoot = resolvePath(options.extractorCodeRoot || path.dirname(extractorRoot));
  const decodedRoot = resolvePath(options.decodedRoot
    || path.join("..", ".codex_tmp", `current-build-${build}-table-extract-candidate`, "Excels"));

  if (options.appmanifest) {
    run("bpsr-steam-patch-gate.mjs", [
      "snapshot",
      "--appmanifest", resolvePath(options.appmanifest),
      "--output", distribution,
      ...optionalPair("--steamdb-change-number", options.steamdbChangeNumber),
      ...optionalPair("--steamdb-build-id", options.steamdbBuildId),
      ...optionalPair("--steamdb-last-record-update", options.steamdbLastRecordUpdate),
    ]);
  } else requireFile(distribution, "distribution snapshot (pass --appmanifest to create it)");

  requireDirectory(extractorRoot, "current-build extractor output");
  requireDirectory(extractorCodeRoot, "BPSR extractor code root");
  requireDirectory(decodedRoot, "current-build decoded table root");
  requireBuildArtifact(
    path.join(extractorRoot, "BattleImagineBehaviorLinks.json"),
    build,
    "current-build Battle Imagine behavior graph",
  );

  const physicalRoot = resolvePath(options.physicalRoot || path.join(buildRoot, "physical", "files"));
  requireDirectory(physicalRoot, "current-build installed-client physical inventory");
  run("bpsr-installed-client-manifest.mjs", [
    "generate", "--build-root", buildRoot,
    "--physical-root", physicalRoot,
    "--distribution", distribution,
    "--output", installedClientManifest,
    ...optionalPair("--depot-manifest", options.depotManifest && resolvePath(options.depotManifest)),
  ]);
  run("bpsr-installed-client-manifest.mjs", [
    "verify", "--manifest", installedClientManifest,
    "--physical-root", physicalRoot,
    "--distribution", distribution,
    ...optionalPair("--depot-manifest", options.depotManifest && resolvePath(options.depotManifest)),
  ]);

  const decodedReferenceGraph = path.join(
    extractorRoot,
    "probing-reports",
    "DecodedTableReferenceGraph.json",
  );
  const decodedReferenceOccurrences = path.join(
    extractorRoot,
    "probing-reports",
    "DecodedTableReferenceGraph.ambiguous-reference-occurrences.jsonl",
  );
  const decodedReferenceCandidates = path.join(
    extractorRoot,
    "probing-reports",
    "DecodedTableReferenceGraph.reference-candidates.jsonl",
  );
  const decodedReferenceCallsiteProofs = path.join(
    extractorRoot,
    "probing-reports",
    "DecodedTableReferenceGraph.callsite-proofs.json",
  );
  const decodedSemanticFieldSchema = path.join(
    extractorRoot,
    "probing-reports",
    "DecodedTableReferenceGraph.semantic-field-schema.v1.json",
  );
  const decodedFieldSchema = path.join(
    extractorRoot,
    "probing-reports",
    "DecodedTableReferenceGraph.decoded-field-schema.v1.json",
  );
  const referenceGapDispositions = path.join(
    buildRoot,
    "reference-gap-dispositions.v1.json",
  );
  const dispositionArgs = existsSync(referenceGapDispositions)
    ? ["--missing-target-dispositions", referenceGapDispositions]
    : [];
  // Pass one emits the complete candidate ledger.  It deliberately promotes
  // nothing from numeric namespace membership alone.
  runExtractor(path.join(extractorCodeRoot, "DecodedTableReferenceGraph.gen"), [
    "--decoded-root", decodedRoot,
    "--build", build,
    "--out", decodedReferenceGraph,
    "--occurrences-out", decodedReferenceOccurrences,
    "--candidates-out", decodedReferenceCandidates,
    ...dispositionArgs,
  ]);
  requireGeneratedBuild(decodedReferenceGraph, build, "decoded table reference graph");
  requireFile(decodedReferenceOccurrences, "decoded table untyped-reference worklist");
  requireFile(decodedReferenceCandidates, "decoded table reference-candidate ledger");

  const existingCallsiteProof = existsSync(decodedReferenceCallsiteProofs)
    ? JSON.parse(readFileSync(decodedReferenceCallsiteProofs, "utf8"))
    : null;
  const gameAssembly = options.gameAssembly
    ? resolvePath(options.gameAssembly)
    : deriveGameAssembly(options.appmanifest)
      || existingCallsiteProof?.inputs?.binary?.path;
  const il2cppDump = options.il2cppDump
    ? resolvePath(options.il2cppDump)
    : existingCallsiteProof?.inputs?.dump?.path
      || resolvePath(path.join("..", ".codex_tmp", `il2cpp-current-${build}-full-output`, "dump.cs"));
  requireFile(gameAssembly, "current-build GameAssembly.dll (pass --game-assembly or --appmanifest)");
  requireFile(il2cppDump, "current-build IL2CPP dump (pass --il2cpp-dump)");
  runPython(path.join(scriptDir, "il2cpp-table-reference-callsite-proof.py"), [
    "--binary", gameAssembly,
    "--dump", il2cppDump,
    "--candidates", decodedReferenceCandidates,
    "--game-build", build,
    "--output", decodedReferenceCallsiteProofs,
  ]);
  requireGeneratedBuild(
    decodedReferenceCallsiteProofs,
    build,
    "current-build IL2CPP table-reference callsite proofs",
  );

  // Pass two accepts only uniquely targeted, instruction-boundary-validated
  // source-value -> target-table lookup chains from the exact current binary.
  runExtractor(path.join(extractorCodeRoot, "DecodedTableReferenceGraph.gen"), [
    "--decoded-root", decodedRoot,
    "--build", build,
    "--out", decodedReferenceGraph,
    "--occurrences-out", decodedReferenceOccurrences,
    "--candidates-out", decodedReferenceCandidates,
    "--callsite-proofs", decodedReferenceCallsiteProofs,
    ...dispositionArgs,
  ]);
  requireGeneratedBuild(decodedReferenceGraph, build, "proof-promoted decoded table reference graph");

  run("bpsr-semantic-field-schema-ledger.mjs", [
    "generate",
    "--graph", decodedReferenceGraph,
    "--candidates", decodedReferenceCandidates,
    "--callsite-proofs", decodedReferenceCallsiteProofs,
    "--il2cpp-dump", il2cppDump,
    "--build", build,
    "--output", decodedSemanticFieldSchema,
  ]);
  run("bpsr-semantic-field-schema-ledger.mjs", [
    "verify", "--input", decodedSemanticFieldSchema,
  ]);
  run("bpsr-decoded-field-schema-manifest.mjs", [
    "generate",
    "--decoded-root", decodedRoot,
    "--semantic-field-schema", decodedSemanticFieldSchema,
    "--il2cpp-dump", il2cppDump,
    "--build", build,
    "--output", decodedFieldSchema,
  ]);
  run("bpsr-decoded-field-schema-manifest.mjs", [
    "verify", "--input", decodedFieldSchema,
  ]);

  run("bpsr-build-source-manifest.mjs", [
    "scan", "--build", build,
    "--extractor-root", extractorRoot,
    "--decoded-root", decodedRoot,
    "--steam-snapshot", distribution,
    "--output", completeManifest,
  ]);
  run("bpsr-build-source-manifest.mjs", [
    "verify", "--manifest", completeManifest,
    "--extractor-root", extractorRoot,
    "--decoded-root", decodedRoot,
  ]);
  run("bpsr-seasonal-domain-scan.mjs", [
    "scan", "--build", build,
    "--extractor-root", extractorRoot,
    "--decoded-root", decodedRoot,
  ]);

  regenerateDamageEvidence({ build, buildRoot, extractorRoot, decodedRoot });
  run("bpsr-cast-recount-relations.mjs", [
    "--skill-table", path.join(decodedRoot, "SkillTable.json"),
    "--skill-effect-table", path.join(decodedRoot, "SkillEffectTable.json"),
    "--skill-fight-level-table", path.join(decodedRoot, "SkillFightLevelTable.json"),
    "--recount-table", path.join(decodedRoot, "RecountTable.json"),
    "--game-build", build,
    "--output", path.join(
      repoRoot,
      "plugins",
      "games",
      "blue-protocol-star-resonance",
      "game-data",
      "runtime",
      "combat-cast-recount-relations.v1.json",
    ),
  ]);

  const semanticConfig = resolvePath(options.semanticConfig || path.join(
    "plugins",
    "games",
    "blue-protocol-star-resonance",
    "research",
    "pipelines",
    "global",
    "current-build-semantic-refresh.config.json",
  ));
  requireFile(semanticConfig, "current-build semantic refresh config");
  run("bpsr-current-build-semantic-refresh.mjs", [
    "refresh",
    "--config", semanticConfig,
    "--build", build,
    "--build-root", buildRoot,
    "--extractor-root", extractorRoot,
    "--decoded-root", decodedRoot,
  ]);

  const effectArgs = ["generate", "--build-root", buildRoot];
  if (options.baselineBuild) {
    const baselineDecodedRoot = resolvePath(options.baselineDecodedRoot
      || path.join("..", ".codex_tmp", `current-build-${options.baselineBuild}-table-extract-candidate`, "Excels"));
    requireDirectory(baselineDecodedRoot, "baseline decoded table root");
    effectArgs.push(
      "--current-buff-table", path.join(decodedRoot, "BuffTable.json"),
      "--baseline-buff-table", path.join(baselineDecodedRoot, "BuffTable.json"),
      "--baseline-build", String(options.baselineBuild),
    );
  }
  run("bpsr-effect-activation-ledger.mjs", effectArgs);
  run("bpsr-unrouted-damage-activation-ledger.mjs", ["generate", "--build-root", buildRoot]);

  if (options.baselineBuild) {
    const baselineRoot = path.join(inventoryRoot, `steam-${options.baselineBuild}`);
    const baselineExtractorRoot = resolvePath(options.baselineExtractorRoot
      || path.join("..", "BPSR-UID-Extractors", `output-build-${options.baselineBuild}-exact`));
    const baselineReferenceGraph = path.join(
      baselineExtractorRoot,
      "probing-reports",
      "DecodedTableReferenceGraph.json",
    );
    const referenceGraphDiff = path.join(
      buildRoot,
      `decoded-reference-graph-diff-from-${options.baselineBuild}.v1.json`,
    );
    const baselineSemanticFieldSchema = path.join(
      baselineExtractorRoot,
      "probing-reports",
      "DecodedTableReferenceGraph.semantic-field-schema.v1.json",
    );
    const semanticFieldSchemaDiff = path.join(
      buildRoot,
      `semantic-field-schema-diff-from-${options.baselineBuild}.v1.json`,
    );
    const baselineDecodedFieldSchema = path.join(
      baselineExtractorRoot,
      "probing-reports",
      "DecodedTableReferenceGraph.decoded-field-schema.v1.json",
    );
    const decodedFieldSchemaDiff = path.join(
      buildRoot,
      `decoded-field-schema-diff-from-${options.baselineBuild}.v1.json`,
    );
    const baselineMechanicClosure = path.join(
      baselineRoot,
      "semantic-mechanic-dependency-closure.v1.json",
    );
    const candidateMechanicClosure = path.join(
      buildRoot,
      "semantic-mechanic-dependency-closure.v1.json",
    );
    const mechanicClosureDiff = path.join(
      buildRoot,
      `semantic-mechanic-dependency-diff-from-${options.baselineBuild}.v1.json`,
    );
    const baselineDistribution = path.join(baselineRoot, "steam-distribution-snapshot.v1.json");
    const baselineManifest = path.join(baselineRoot, "complete-build-source-manifest.v1.json");
    requireDirectory(baselineRoot, "baseline build evidence root");
    if (existsSync(baselineReferenceGraph)) {
      run("bpsr-decoded-reference-graph-diff.mjs", [
        "diff", "--baseline", baselineReferenceGraph,
        "--candidate", decodedReferenceGraph,
        "--output", referenceGraphDiff,
      ]);
      run("bpsr-decoded-reference-graph-diff.mjs", ["verify", "--input", referenceGraphDiff]);
    } else {
      console.warn(
        `Legacy baseline ${options.baselineBuild} has no decoded reference graph; `
        + "the current graph is complete, but exact row/relationship deltas are unavailable for this one transition.",
      );
    }
    if (existsSync(baselineSemanticFieldSchema)) {
      run("bpsr-semantic-field-schema-ledger.mjs", [
        "diff",
        "--baseline", baselineSemanticFieldSchema,
        "--candidate", decodedSemanticFieldSchema,
        "--output", semanticFieldSchemaDiff,
      ]);
    } else {
      console.warn(
        `Legacy baseline ${options.baselineBuild} has no semantic field-schema ledger; `
        + "future transitions will include exact field-domain changes after this baseline.",
      );
    }
    if (existsSync(baselineDecodedFieldSchema)) {
      run("bpsr-decoded-field-schema-manifest.mjs", [
        "diff",
        "--baseline", baselineDecodedFieldSchema,
        "--candidate", decodedFieldSchema,
        "--output", decodedFieldSchemaDiff,
      ]);
    } else {
      console.warn(
        `Legacy baseline ${options.baselineBuild} has no universal decoded field-schema manifest; `
        + "future transitions will include exact scalar, array, and relationship-field deltas after this baseline.",
      );
    }
    if (existsSync(baselineMechanicClosure)) {
      requireFile(candidateMechanicClosure, "candidate semantic mechanic dependency closure");
      run("bpsr-semantic-mechanic-dependency-closure.mjs", [
        "diff",
        "--baseline", baselineMechanicClosure,
        "--candidate", candidateMechanicClosure,
        "--output", mechanicClosureDiff,
      ]);
    } else {
      console.warn(
        `Legacy baseline ${options.baselineBuild} has no semantic mechanic dependency closure; `
        + "future transitions will compare every mechanic source, decoded row, field, relationship, and unresolved proof requirement.",
      );
    }
    if (existsSync(baselineDistribution)) {
      run("bpsr-steam-patch-gate.mjs", [
        "diff", "--baseline", baselineDistribution, "--candidate", distribution,
        "--output", path.join(buildRoot, `distribution-diff-from-${options.baselineBuild}.v1.json`),
      ]);
    } else {
      console.warn(`Legacy baseline ${options.baselineBuild} has no Steam distribution snapshot; depot-level diff is unavailable for this one transition.`);
    }
    if (existsSync(baselineManifest)) {
      run("bpsr-build-source-manifest.mjs", [
        "diff", "--baseline", baselineManifest, "--candidate", completeManifest,
        "--output", path.join(buildRoot, `complete-source-diff-from-${options.baselineBuild}.v1.json`),
      ]);
    } else {
      console.warn(`Legacy baseline ${options.baselineBuild} has no complete source manifest; file-level diff is unavailable for this one transition.`);
    }
    const seasonalDiff = path.join(
      buildRoot,
      "seasonal-domains",
      `diff-from-${options.baselineBuild}.v1.json`,
    );
    const seasonalDiffStatus = runAllowExitCodes("bpsr-seasonal-domain-scan.mjs", [
      "diff", "--baseline-build", options.baselineBuild, "--candidate-build", build,
      "--output", seasonalDiff,
    ], [0, 2]);
    requireFile(seasonalDiff, "seasonal-domain diff output");
    if (seasonalDiffStatus === 2) {
      console.warn(
        `Legacy baseline ${options.baselineBuild} is missing one or more newer seasonal-domain manifests; `
        + `the omissions remain explicit in ${seasonalDiff}, while the current build still requires all domains.`,
      );
    }

    if (options.baselineDepotManifest && options.depotManifest) {
      const physicalDiff = path.join(buildRoot, "client-source-diff.json");
      const depotManifestDiff = path.join(
        buildRoot,
        `depot-manifest-diff-from-${options.baselineBuild}.v1.json`,
      );
      const rescanPlan = path.join(
        buildRoot,
        `patch-rescan-plan-from-${options.baselineBuild}.v1.json`,
      );
      requireFile(physicalDiff, "installed-client physical diff");
      runDepotManifestDiff([
        "diff",
        "--old", resolvePath(options.baselineDepotManifest),
        "--new", resolvePath(options.depotManifest),
        "--output", depotManifestDiff,
      ]);
      run("bpsr-patch-rescan-plan.mjs", [
        "generate",
        "--manifest-diff", depotManifestDiff,
        "--installed-manifest", installedClientManifest,
        "--physical-diff", physicalDiff,
        "--source-manifest", completeManifest,
        "--semantic-diff", seasonalDiff,
        ...optionalPair("--reference-graph-diff", existsSync(referenceGraphDiff) ? referenceGraphDiff : undefined),
        ...optionalPair("--semantic-field-diff", existsSync(semanticFieldSchemaDiff) ? semanticFieldSchemaDiff : undefined),
        ...optionalPair("--decoded-field-diff", existsSync(decodedFieldSchemaDiff) ? decodedFieldSchemaDiff : undefined),
        ...optionalPair("--mechanic-dependency-diff", existsSync(mechanicClosureDiff) ? mechanicClosureDiff : undefined),
        "--output", rescanPlan,
      ]);
      run("bpsr-patch-rescan-plan.mjs", ["verify", "--input", rescanPlan]);
    } else if (options.baselineDepotManifest) {
      console.warn(
        "Both --baseline-depot-manifest and --depot-manifest are required for the offline changed-file fast path.",
      );
    }
  }

  run("bpsr-current-build-unmapped-catalog.mjs", [
    "generate", "--build-root", buildRoot,
    "--reference-graph", decodedReferenceGraph,
    "--semantic-field-schema", decodedSemanticFieldSchema,
    "--decoded-field-schema", decodedFieldSchema,
  ]);
  run("bpsr-current-build-completeness.mjs", [
    "generate", "--build-root", buildRoot,
    "--reference-graph", decodedReferenceGraph,
    "--semantic-field-schema", decodedSemanticFieldSchema,
    "--decoded-field-schema", decodedFieldSchema,
  ]);
  console.log(`Build ${build} baseline refreshed and verified without silent omissions.`);
}

function regenerateDamageEvidence({ build, buildRoot, extractorRoot, decodedRoot }) {
  const surface = path.join(extractorRoot, "DamageFormulaSurface.json");
  const il2cppSurface = path.join(buildRoot, "il2cpp-combat-surface.v2.json");
  const worklist = path.join(buildRoot, "ctb-rdps-proof-worklist.json");
  const routeProof = path.join(buildRoot, "damage-source-route-proof.candidate.v9.json");
  const stageCatalog = path.join(buildRoot, "damage-stage-rdps.candidate.v14.json");
  const familyWorklist = path.join(buildRoot, "damage-script-family-worklist.v6.json");
  const referenceScan = path.join(buildRoot, "decoded-table-reference-scan.v3.json");
  const resolutionLedger = path.join(buildRoot, "damage-resolution-ledger.v2.json");

  [surface, il2cppSurface, worklist].forEach((file) => requireFile(file, "damage evidence input"));

  runCargoGenerated("rlogs-bpsr-damage-source-route-proof", [
    "--surface", surface,
    "--damage-attr-table", path.join(decodedRoot, "DamageAttrTable.json"),
    "--bullet-table", path.join(decodedRoot, "BulletTable.json"),
    "--bullet-run-table", path.join(decodedRoot, "BulletRunTable.json"),
    "--bullet-shape-table", path.join(decodedRoot, "BulletShapeTable.json"),
    "--buff-table", path.join(decodedRoot, "BuffTable.json"),
    "--skill-table", path.join(decodedRoot, "SkillTable.json"),
    "--skill-effect-table", path.join(decodedRoot, "SkillEffectTable.json"),
    "--skill-fight-level-table", path.join(decodedRoot, "SkillFightLevelTable.json"),
    "--recount-table", path.join(decodedRoot, "RecountTable.json"),
    "--il2cpp-surface", il2cppSurface,
    "--build", build,
  ], routeProof);

  runCargoGenerated("rlogs-bpsr-damage-stage-runtime-catalog", [
    "--surface", surface,
    "--decoded-table", path.join(decodedRoot, "DamageAttrTable.json"),
    "--route-proof", routeProof,
    "--build", build,
  ], stageCatalog);

  runCargoGenerated("rlogs-bpsr-damage-script-family-worklist", [
    "--catalog", stageCatalog,
    "--route-proof", routeProof,
    "--build", build,
  ], familyWorklist);

  runCargoGenerated("rlogs-bpsr-decoded-table-reference-scan", [
    "--decoded-root", decodedRoot,
    "--worklist", worklist,
    "--route-proof", routeProof,
    "--build", build,
  ], referenceScan);

  runCargoGenerated("rlogs-bpsr-damage-resolution-ledger", [
    "--route-proof", routeProof,
    "--stage-catalog", stageCatalog,
    "--family-worklist", familyWorklist,
    "--reference-scan", referenceScan,
    "--build", build,
  ], resolutionLedger);
}

function run(script, args) {
  execFileSync(process.execPath, [path.join(scriptDir, script), ...args], {
    cwd: repoRoot,
    stdio: "inherit",
  });
}

function runExtractor(script, args) {
  requireFile(script, "BPSR extractor generator");
  execFileSync(process.execPath, [script, ...args], {
    cwd: path.dirname(script),
    stdio: "inherit",
  });
}

function runPython(script, args) {
  requireFile(script, "Python proof scanner");
  execFileSync("python", [script, ...args], {
    cwd: repoRoot,
    stdio: "inherit",
  });
}

function deriveGameAssembly(appmanifest) {
  if (!appmanifest) return null;
  const manifestPath = resolvePath(appmanifest);
  requireFile(manifestPath, "Steam appmanifest");
  const text = readFileSync(manifestPath, "utf8");
  const installDir = text.match(/"installdir"\s+"([^"]+)"/i)?.[1];
  if (!installDir) throw new Error(`Cannot read installdir from ${manifestPath}`);
  return path.join(path.dirname(manifestPath), "common", installDir, "bpsr", "GameAssembly.dll");
}

function runAllowExitCodes(script, args, allowedExitCodes) {
  try {
    run(script, args);
    return 0;
  } catch (error) {
    const status = Number(error?.status);
    if (!allowedExitCodes.includes(status)) throw error;
    return status;
  }
}

function runDepotManifestDiff(args) {
  execFileSync("dotnet", [
    "run",
    "--project", path.join(scriptDir, "steam-depot-manifest-diff", "SteamDepotManifestDiff.csproj"),
    "--configuration", "Release",
    "--",
    ...args,
  ], { cwd: repoRoot, stdio: "inherit" });
}

function runCargoGenerated(binary, args, output) {
  const next = `${output}.next`;
  if (existsSync(next)) unlinkSync(next);
  execFileSync("cargo", [
    "run", "--quiet", "-p", "rlogs-game-bpsr", "--bin", binary, "--",
    ...args, "--output", next,
  ], { cwd: repoRoot, stdio: "inherit" });
  requireFile(next, `${binary} candidate output`);
  copyFileSync(next, output);
  unlinkSync(next);
}

function optionalPair(flag, value) { return value === undefined ? [] : [flag, String(value)]; }
function requireFile(file, label) {
  if (!file || !existsSync(file)) throw new Error(`Missing ${label}: ${file || "<not provided>"}`);
}
function requireDirectory(directory, label) { requireFile(directory, label); }
function requireBuildArtifact(file, build, label) {
  requireFile(file, label);
  const artifact = JSON.parse(readFileSync(file, "utf8"));
  const artifactBuild = String(artifact?.summary?.gameBuild || "");
  if (artifactBuild !== String(build)) {
    throw new Error(`${label} is for build ${artifactBuild || "<missing>"}, expected ${build}: ${file}`);
  }
  if ((artifact?.summary?.parseFailures || []).length) {
    throw new Error(`${label} contains parse failures and cannot be promoted: ${file}`);
  }
}
function requireGeneratedBuild(file, build, label) {
  requireFile(file, label);
  const artifact = JSON.parse(readFileSync(file, "utf8"));
  const artifactBuild = String(
    artifact?.game_build
      ?? artifact?.current_game_build
      ?? artifact?.static_game_build
      ?? artifact?.build_id
      ?? "",
  );
  if (artifactBuild !== String(build)) {
    throw new Error(`${label} is for build ${artifactBuild || "<missing>"}, expected ${build}: ${file}`);
  }
}
function required(value, key) { if (!value[key]) throw new Error(`Missing --${toKebab(key)}`); return String(value[key]); }
function resolvePath(value) { return path.isAbsolute(value) ? value : path.resolve(repoRoot, value); }
function toKebab(value) { return value.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`); }
function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const key = token.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    const next = args[index + 1];
    if (!next || next.startsWith("--")) throw new Error(`Missing value for ${token}`);
    output[key] = next;
    index += 1;
  }
  return output;
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-refresh-build-baseline.mjs refresh --build <id> [--appmanifest <acf>] [--game-assembly <dll>] [--il2cpp-dump <dump.cs>] [--extractor-root <path>] [--extractor-code-root <path>] [--decoded-root <path>] [--semantic-config <path>] [--baseline-build <id>] [--baseline-decoded-root <path>] [--baseline-extractor-root <path>] [--depot-manifest <path>] [--baseline-depot-manifest <path>] [--steamdb-change-number <id>] [--steamdb-build-id <id>] [--steamdb-last-record-update <date>]

The extractor and decoded-table stages deliberately remain outside the live parser. This command inventories their complete outputs, verifies zero omissions, creates semantic domain fingerprints, regenerates the build-locked damage route/formula/reference/resolution chain and semantic rDPS worklist/audit/formula/recipient/preflight layers, correlates preserved effect and damage definitions with indexed packet activation evidence, optionally carries forward byte-identical packet-proven relationship lineage and diffs a prior build, compares cached Steam depot manifests into an exact changed-file rescan plan, then regenerates the proof-blocker report.`);
  process.exit(exitCode);
}
