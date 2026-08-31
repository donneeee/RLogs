#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDir, "..");
const EXPECTED_REPLAY_AUDIT_SCHEMA = 27;
const [command = "help", ...tokens] = process.argv.slice(2);
const options = parseArgs(tokens);

if (command === "refresh") refresh(resolveContext(options));
else if (command === "verify") verify(resolveContext(options));
else usage(command === "help" ? 0 : 1);

function resolveContext(options) {
  const build = required(options, "build");
  if (!/^\d+$/.test(build)) throw new Error("--build must contain only ASCII digits");
  const deployment = String(options.deployment || "global");
  const outputDir = resolvePath(options["output-dir"] ||
    `runtime-data/research/rdps/steam-${build}`);
  const rlogDir = resolvePath(options["rlog-dir"] || "runtime-data/logs");
  const runtime = resolvePath(options.runtime ||
    "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-formula-runtime.v1.json");
  return {
    build,
    deployment,
    outputDir,
    rlogDir,
    runtime,
    auditInput: optionalPath(options.audit),
    sourceAudit: optionalPath(options["source-audit"]),
    rlogs: (options.rlog || []).map(resolvePath),
    reconciliationInputs: {
      formulas: optionalPath(options.formulas),
      sources: optionalPath(options.sources),
      damageSurface: optionalPath(options["damage-surface"]),
      damageChains: optionalPath(options["damage-chains"]),
      skillNames: optionalPath(options["skill-names"]),
      dreamscope: optionalPath(options.dreamscope),
      moduleTerminals: optionalPath(options["module-terminals"]),
    },
  };
}

function refresh(context) {
  const previous = beginPromotion(context.outputDir);
  try {
    const files = artifactPaths(context.outputDir);
    const sourceRlogs = resolveSourceRlogs(context);
    mkdirSync(context.outputDir, { recursive: true });

    if (context.auditInput) {
      requireFile(context.auditInput, "reused effect audit");
      copyFileSync(context.auditInput, files.audit);
    } else {
      const cargoArgs = sourceRlogs.flatMap((file) => ["--rlog", file]);
      cargoArgs.push("--output", files.audit);
      runCargo("rlogs-bpsr-rdps-effect-audit", cargoArgs);
    }
    const audit = verifyAudit(files.audit, context, sourceRlogs);

    runCargo("rlogs-bpsr-origin-catalog", [files.audit, files.origin]);
    const reconciliationArgs = ["--audit", files.origin, "--output", files.reconciliation];
    for (const [key, value] of Object.entries(context.reconciliationInputs)) {
      if (value) reconciliationArgs.push(`--${key}`, value);
    }
    execNode("rdps-observed-effect-reconciliation.mjs", reconciliationArgs);
    execNode("rdps-external-effect-frontier.mjs", [
      "--reconciliation", files.reconciliation,
      "--runtime", context.runtime,
      "--output", files.frontier,
    ]);
    const replayArgs = sourceRlogs.flatMap((file) => ["--rlog", file]);
    replayArgs.push("--summary-only", "--output", files.replay);
    runCargo("rlogs-bpsr-rdps-replay-audit", replayArgs);

    const verification = verifyGeneratedArtifacts(context, files, audit, sourceRlogs);
    const manifest = buildManifest(context, files, sourceRlogs, verification, previous);
    writeJson(files.manifest, manifest);
    verifyManifest(context, files);
    console.log(JSON.stringify({
      output_directory: relativePath(context.outputDir),
      previous_output_backup: previous ? relativePath(previous) : null,
      ...verification.summary,
    }, null, 2));
  } catch (error) {
    rollbackPromotion(context.outputDir, previous);
    throw error;
  }
}

function verify(context) {
  const files = artifactPaths(context.outputDir);
  const manifest = verifyManifest(context, files);
  const sourceRlogs = manifest.sources.map((source) => resolvePath(source.path));
  const audit = verifyAudit(files.audit, context, sourceRlogs);
  const verification = verifyGeneratedArtifacts(context, files, audit, sourceRlogs);
  console.log(JSON.stringify({
    verified: relativePath(context.outputDir),
    ...verification.summary,
  }, null, 2));
}

function resolveSourceRlogs(context) {
  if (context.rlogs.length > 0 && context.sourceAudit) {
    throw new Error("Use either repeatable --rlog values or --source-audit, not both");
  }
  if (context.rlogs.length > 0) return uniqueFiles(context.rlogs);

  const sourceAuditPath = context.sourceAudit || context.auditInput;
  if (!sourceAuditPath) {
    throw new Error("Refresh needs repeatable --rlog values, --source-audit, or --audit");
  }
  const audit = readJson(sourceAuditPath, "source audit");
  if (!Array.isArray(audit.sources) || audit.sources.length === 0) {
    throw new Error(`${sourceAuditPath} has no exact source inventory`);
  }
  const files = audit.sources.map((source) => {
    if (!source.file_name || path.basename(source.file_name) !== source.file_name) {
      throw new Error(`Unsafe source audit file_name: ${source.file_name}`);
    }
    return path.join(context.rlogDir, source.file_name);
  });
  return uniqueFiles(files);
}

function verifyAudit(file, context, sourceRlogs) {
  const audit = readJson(file, "effect audit");
  if (Number(audit.schema_version) !== 7) {
    throw new Error(`Effect audit schema ${audit.schema_version} is unsupported; expected 7`);
  }
  if (String(audit.client_build) !== context.build) {
    throw new Error(`Effect audit build ${audit.client_build} does not match ${context.build}`);
  }
  if (String(audit.deployment_id) !== context.deployment) {
    throw new Error(`Effect audit deployment ${audit.deployment_id} does not match ${context.deployment}`);
  }
  if (!Array.isArray(audit.sources) || !Array.isArray(audit.reports)
      || audit.sources.length === 0 || audit.sources.length !== audit.reports.length) {
    throw new Error("Effect audit source/report cardinality is invalid");
  }
  const expectedNames = sourceRlogs.map((filePath) => path.basename(filePath));
  const actualNames = audit.sources.map((source) => source.file_name);
  if (new Set(actualNames).size !== actualNames.length
      || JSON.stringify(actualNames) !== JSON.stringify(expectedNames)) {
    throw new Error("Effect audit source order does not match the exact requested source set");
  }
  for (const source of audit.sources) {
    if (String(source.client_build) !== context.build
        || String(source.deployment_id) !== context.deployment) {
      throw new Error(`Audit source ${source.file_name} has mixed build or deployment identity`);
    }
  }
  return audit;
}

function verifyGeneratedArtifacts(context, files, audit, sourceRlogs) {
  requireFile(context.runtime, "rDPS runtime configuration");
  const runtime = readJson(context.runtime, "rDPS runtime configuration");
  if (String(runtime.game_build) !== context.build) {
    throw new Error(`Runtime build ${runtime.game_build} does not match ${context.build}`);
  }
  execNode("rdps-observed-effect-reconciliation.mjs", ["--verify", files.reconciliation]);
  execNode("rdps-external-effect-frontier.mjs", ["--verify", files.frontier]);

  const origin = readJson(files.origin, "packet-origin catalog");
  const reconciliation = readJson(files.reconciliation, "effect reconciliation");
  const frontier = readJson(files.frontier, "external-effect frontier");
  const replay = verifyReplayAudit(files.replay, context, sourceRlogs, frontier);
  if (String(origin.game_build) !== context.build
      || String(reconciliation.game_build) !== context.build
      || String(frontier.game_build) !== context.build) {
    throw new Error("A generated artifact does not match the requested game build");
  }
  const observedEffects = Number(origin.summary?.observed_effects);
  if (Number(origin.summary?.source_sessions) !== audit.reports.length
      || observedEffects !== origin.effects?.length
      || observedEffects !== reconciliation.effects?.length
      || observedEffects !== Number(reconciliation.summary?.observed_effects)
      || observedEffects !== Number(frontier.summary?.all_observed_effects)) {
    throw new Error("Cross-artifact source/effect conservation failed");
  }
  if (Number(frontier.summary?.external_candidates)
      !== Number(frontier.summary?.external_candidates_retained)
      || frontier.summary?.zero_hidden_omissions !== true
      || frontier.policy?.zero_hidden_omissions !== true) {
    throw new Error("External frontier does not retain every attribution candidate");
  }
  if (sourceRlogs.length !== audit.sources.length) {
    throw new Error("Source file conservation failed");
  }
  return {
    origin,
    reconciliation,
    frontier,
    summary: {
      build: context.build,
      deployment: context.deployment,
      sealed_sources: audit.sources.length,
      observed_effects: observedEffects,
      external_candidates: frontier.summary.external_candidates,
      ready_for_damage_attribution: frontier.summary.ready_for_damage_attribution,
      conserved_unattributed_damage_candidates:
        frontier.summary.conserved_unattributed_damage_candidates,
      retained_support_candidates: frontier.summary.retained_support_candidates,
      retained_unclassified_candidates: frontier.summary.retained_unclassified_candidates,
      replayed_sources: replay.reports.length,
      replayed_events: replay.total_events,
      replay_events_per_second: replay.events_per_second,
      replay_runtime_target_matches: replay.runtimeTargetMatches,
      replay_runtime_target_mismatches: replay.reports.length - replay.runtimeTargetMatches,
      runtime_attribution_allowed:
        replay.reports.length > 0 && replay.runtimeTargetMatches === replay.reports.length,
      replay_contribution_effect_ids: replay.contributionEffectIds,
      replay_contribution_events: replay.contributionEvents,
      replay_attributed_bonus_damage: replay.attributedBonusDamage,
      zero_hidden_omissions: true,
    },
  };
}

function verifyReplayAudit(file, context, sourceRlogs, frontier) {
  const replay = readJson(file, "sealed rDPS replay audit");
  if (Number(replay.schema_version) !== EXPECTED_REPLAY_AUDIT_SCHEMA) {
    throw new Error(
      `rDPS replay audit schema ${replay.schema_version} is unsupported; `
        + `expected ${EXPECTED_REPLAY_AUDIT_SCHEMA}`,
    );
  }
  if (String(replay.runtime_rule_build) !== context.build
      || String(replay.runtime_rule_deployment) !== context.deployment) {
    throw new Error("rDPS replay runtime identity does not match the requested target");
  }
  if (!Array.isArray(replay.reports) || replay.reports.length !== sourceRlogs.length) {
    throw new Error("rDPS replay source/report cardinality is invalid");
  }
  const expectedNames = sourceRlogs.map((file) => path.basename(file)).sort();
  const actualNames = replay.reports.map((report) => path.basename(report.source_path)).sort();
  if (JSON.stringify(actualNames) !== JSON.stringify(expectedNames)) {
    throw new Error("rDPS replay source set does not match the exact requested source set");
  }
  for (const report of replay.reports) {
    if (String(report.client_build) !== context.build
        || String(report.deployment_id) !== context.deployment) {
      throw new Error(`rDPS replay build/deployment mismatch: ${report.source_path}`);
    }
    if (typeof report.runtime_target_match !== "boolean") {
      throw new Error(`rDPS replay omitted runtime promotion state: ${report.source_path}`);
    }
    if (report.conserved !== true) {
      throw new Error(`rDPS replay failed damage conservation: ${report.source_path}`);
    }
  }
  const enabledEffectIds = new Set(replay.rule_effect_ids.map(Number));
  const readyEffectIds = frontier.effects
    .filter((effect) => effect.promotion_state === "ready-for-damage-attribution")
    .map((effect) => Number(effect.effect_id));
  const runtimeTargetMatches = replay.reports.filter((report) => report.runtime_target_match).length;
  const runtimeAttributionAllowed = replay.reports.length > 0
    && runtimeTargetMatches === replay.reports.length;
  if (runtimeAttributionAllowed) {
    for (const effectId of readyEffectIds) {
      if (!enabledEffectIds.has(effectId)) {
        throw new Error(`Ready effect ${effectId} is absent from the shared history/live projector`);
      }
    }
  }
  const contributionCounts = new Map();
  let attributedBonusDamage = 0;
  for (const report of replay.reports) {
    attributedBonusDamage += Number(report.summary?.attributed_bonus_damage || 0);
    for (const [effectId, count] of Object.entries(
      report.emitted_contribution_events_by_effect || {},
    )) {
      contributionCounts.set(Number(effectId), (contributionCounts.get(Number(effectId)) || 0)
        + Number(count));
    }
  }
  return {
    ...replay,
    runtimeTargetMatches,
    contributionEffectIds: [...contributionCounts.keys()].sort((left, right) => left - right),
    contributionEvents: [...contributionCounts.values()].reduce((sum, count) => sum + count, 0),
    attributedBonusDamage,
  };
}

function buildManifest(context, files, sourceRlogs, verification, previous) {
  const artifactEntries = [
    ["effect_audit", files.audit],
    ["packet_origin_catalog", files.origin],
    ["effect_reconciliation", files.reconciliation],
    ["external_effect_frontier", files.frontier],
    ["sealed_rdps_replay_audit", files.replay],
  ].map(([id, file]) => ({
    id,
    path: relativePath(file),
    bytes: statSync(file).size,
    sha256: sha256(file),
    schema_version: readJson(file, id).schema_version,
  }));
  return {
    schema_version: 1,
    game: "blue-protocol-star-resonance",
    game_build: context.build,
    deployment_id: context.deployment,
    generated_at_utc: new Date().toISOString(),
    generated_by: "tools/bpsr-current-build-observed-effect-refresh.mjs",
    policy: {
      exact_sealed_source_set_only: true,
      mixed_build_or_deployment_inputs_rejected: true,
      generated_outputs_verified_before_success: true,
      prior_output_retained_as_rollback_backup: true,
      unresolved_packet_evidence_retained: true,
      no_candidate_rule_automatically_enabled: true,
      history_and_live_share_versioned_runtime_rules: true,
      runtime_promotion_state_is_recorded_not_assumed: true,
      sealed_replay_conservation_verified: true,
    },
    previous_output_backup: previous ? relativePath(previous) : null,
    runtime_configuration: fileIdentity(context.runtime),
    sources: sourceRlogs.map(fileIdentity),
    artifacts: artifactEntries,
    promotion_summary: verification.summary,
  };
}

function verifyManifest(context, files) {
  const manifest = readJson(files.manifest, "observed-effect refresh manifest");
  if (Number(manifest.schema_version) !== 1
      || manifest.generated_by !== "tools/bpsr-current-build-observed-effect-refresh.mjs"
      || String(manifest.game_build) !== context.build
      || String(manifest.deployment_id) !== context.deployment) {
    throw new Error("Observed-effect refresh manifest identity is invalid");
  }
  if (!manifest.policy?.unresolved_packet_evidence_retained
      || !manifest.policy?.no_candidate_rule_automatically_enabled
      || !manifest.policy?.runtime_promotion_state_is_recorded_not_assumed
      || !manifest.policy?.sealed_replay_conservation_verified
      || !manifest.promotion_summary?.zero_hidden_omissions) {
    throw new Error("Observed-effect refresh manifest safety policy is invalid");
  }
  for (const entry of [...manifest.sources, manifest.runtime_configuration, ...manifest.artifacts]) {
    const file = resolvePath(entry.path);
    requireFile(file, entry.path);
    if (statSync(file).size !== entry.bytes || sha256(file) !== entry.sha256) {
      throw new Error(`Manifest hash mismatch: ${entry.path}`);
    }
  }
  return manifest;
}

function artifactPaths(outputDir) {
  return {
    audit: path.join(outputDir, "observed-effect-audit.v1.json"),
    origin: path.join(outputDir, "observed-effect-origin-catalog.v1.json"),
    reconciliation: path.join(outputDir, "observed-effect-reconciliation.v1.json"),
    frontier: path.join(outputDir, "external-effect-frontier.v1.json"),
    replay: path.join(outputDir, "rdps-replay-audit.v1.json"),
    manifest: path.join(outputDir, "observed-effect-refresh-manifest.v1.json"),
  };
}

function beginPromotion(outputDir) {
  if (!existsSync(outputDir)) return null;
  const backup = `${outputDir}.previous-${timestamp()}`;
  renameSync(outputDir, backup);
  return backup;
}

function rollbackPromotion(outputDir, previous) {
  if (existsSync(outputDir)) renameSync(outputDir, `${outputDir}.failed-${timestamp()}`);
  if (previous && existsSync(previous)) renameSync(previous, outputDir);
}

function runCargo(binary, args) {
  execFileSync("cargo", [
    "run", "--quiet", "-p", "rlogs-game-bpsr", "--bin", binary, "--", ...args,
  ], { cwd: repositoryRoot, stdio: "inherit" });
}

function execNode(script, args) {
  execFileSync(process.execPath, [path.join(scriptDir, script), ...args], {
    cwd: repositoryRoot,
    stdio: "inherit",
  });
}

function parseArgs(tokens) {
  const result = {};
  for (let index = 0; index < tokens.length; index += 2) {
    const token = tokens[index];
    const value = tokens[index + 1];
    if (!token?.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    if (!value || value.startsWith("--")) throw new Error(`Missing value for ${token}`);
    const key = token.slice(2);
    if (key === "rlog") (result.rlog ||= []).push(value);
    else if (result[key] !== undefined) throw new Error(`Duplicate --${key}`);
    else result[key] = value;
  }
  return result;
}

function uniqueFiles(files) {
  const normalized = files.map((file) => path.normalize(file));
  if (new Set(normalized.map((file) => file.toLowerCase())).size !== normalized.length) {
    throw new Error("The exact rlog source set contains duplicate paths");
  }
  normalized.forEach((file) => requireFile(file, "sealed rlog source"));
  return normalized;
}

function fileIdentity(file) {
  requireFile(file, "manifest input");
  return { path: relativePath(file), bytes: statSync(file).size, sha256: sha256(file) };
}

function required(options, key) {
  if (!options[key]) throw new Error(`Missing --${key}`);
  return String(options[key]);
}

function optionalPath(value) { return value ? resolvePath(value) : null; }
function resolvePath(value) {
  return path.isAbsolute(value) ? path.normalize(value) : path.resolve(repositoryRoot, value);
}
function relativePath(value) { return path.relative(repositoryRoot, value).replaceAll("\\", "/"); }
function readJson(file, label) {
  requireFile(file, label);
  return JSON.parse(readFileSync(file, "utf8"));
}
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`); }
function requireFile(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); }
function sha256(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function timestamp() { return new Date().toISOString().replaceAll(":", "-").replaceAll(".", "-"); }

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-current-build-observed-effect-refresh.mjs refresh --build <id> [--deployment global]
    (--audit <existing-audit.json> | --source-audit <existing-audit.json> | --rlog <sealed.rlog> [--rlog ...])
    [--rlog-dir runtime-data/logs] [--output-dir <directory>] [reconciliation input overrides]

  node tools/bpsr-current-build-observed-effect-refresh.mjs verify --build <id>
    [--deployment global] [--output-dir <directory>]

The refresh uses one explicit homogeneous sealed-rlog corpus, rebuilds the packet-origin catalog,
reconciles every observed effect with exact-build static evidence, builds the external attribution
frontier, verifies conservation, and records content hashes. It never enables an rDPS rule merely
because it appears in the candidate frontier. Existing output is retained as a rollback backup.`);
  process.exit(exitCode);
}
