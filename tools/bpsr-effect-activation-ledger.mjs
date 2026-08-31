#!/usr/bin/env node

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const buildRoot = resolvePath(required(options, "buildRoot"));
  const reachabilityPath = path.join(buildRoot, "equipment-set-child-buff-reachability.v1.json");
  const scopePath = path.join(buildRoot, "rdps-recipient-scope-ledger.v2.json");
  const outputPath = resolvePath(options.output ?? path.join(buildRoot, "effect-activation-ledger.v1.json"));
  requireFile(reachabilityPath);
  requireFile(scopePath);

  const reachability = readJson(reachabilityPath);
  const scope = readJson(scopePath);
  const buildId = String(reachability.gameBuild);
  assert(String(scope.static_game_build) === buildId, "Static build identity mismatch");

  const lineage = loadRelationshipLineage(options, buildId);

  const packetEdgesByChild = collectPacketEdges(scope.candidates ?? []);
  const effects = Object.values(reachability.effectsById ?? {})
    .map((entry) => buildEffectEntry(entry, packetEdgesByChild, scope, lineage))
    .sort((left, right) => left.effect_id - right.effect_id);

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-effect-activation-ledger.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    game_build: buildId,
    packet_evidence_build: String(scope.historical_packet_build ?? "unknown"),
    policy: {
      current_build_static_definition_is_not_runtime_activation_proof: true,
      unchanged_exact_parent_and_child_rows_may_carry_packet_proven_relationship_identity_forward: true,
      carried_relationship_identity_is_not_matching_build_runtime_activation_or_formula_proof: true,
      unobserved_definitions_remain_visible_and_diffable: true,
      only_packet_observed_or_currently_referenced_effects_are_exact_relationship_blockers: true,
      dormant_definitions_never_feed_runtime_rdps: true,
    },
    summary: summarize(effects),
    effects,
    inputs: {
      static_reachability: relativeRepo(reachabilityPath),
      recipient_scope: relativeRepo(scopePath),
      current_buff_table: lineage ? relativeRepo(lineage.currentPath) : null,
      baseline_buff_table: lineage ? relativeRepo(lineage.baselinePath) : null,
      relationship_lineage_baseline_build: lineage?.baselineBuild ?? null,
    },
  };

  writeJson(outputPath, report);
  console.log(`Effect activation ledger for build ${buildId}: ${effects.length} definitions.`);
  console.log(`Packet-observed family children: ${report.summary.packet_observed_family_children}.`);
  console.log(`Definition-only and unobserved: ${report.summary.definition_only_unobserved}.`);
  console.log(`Current-build relationship blockers: ${report.summary.current_build_relationship_blockers}.`);
  console.log(`Wrote ${relativeRepo(outputPath)}`);
}

function buildEffectEntry(entry, packetEdgesByChild, scope, lineage) {
  const effectId = Number(entry.effectId);
  const packetEdges = packetEdgesByChild.get(effectId) ?? [];
  const hasCurrentStaticIncomingReference = [
    entry.tableIncomingReferences,
    entry.clientAssetReferences,
    entry.unparsedClientAssetReferences,
    entry.semanticCodeTokenReferences,
  ].some((values) => (values ?? []).length > 0);
  const hasHistoricalPacketEdge = packetEdges.length > 0;
  const staticDefinitionOnly = entry.reachabilityStatus === "definition-only-no-current-incoming-reference";
  const lineageEdges = packetEdges.map((edge) => compareEdgeLineage(edge, lineage));
  const relationshipLineageCarriedForward = hasHistoricalPacketEdge
    && lineageEdges.every((edge) => edge.exact_parent_and_child_rows_unchanged === true);

  let activationStatus = "current-static-reference-needs-runtime-proof";
  if (staticDefinitionOnly && hasHistoricalPacketEdge) {
    activationStatus = "historical-packet-child-current-definition-only";
  } else if (staticDefinitionOnly) {
    activationStatus = "definition-only-unobserved-in-indexed-packet-corpus";
  } else if (!hasCurrentStaticIncomingReference && !hasHistoricalPacketEdge) {
    activationStatus = "no-current-reference-and-unobserved";
  }
  if (relationshipLineageCarriedForward) {
    activationStatus = "unchanged-packet-proved-relationship-lineage";
  }

  const currentBuildRelationshipProven = relationshipLineageCarriedForward;
  return {
    effect_id: effectId,
    static_reachability_status: entry.reachabilityStatus,
    activation_status: activationStatus,
    has_current_static_incoming_reference: hasCurrentStaticIncomingReference,
    has_historical_packet_family_edge: hasHistoricalPacketEdge,
    relationship_lineage_carried_forward: relationshipLineageCarriedForward,
    current_build_relationship_proven: currentBuildRelationshipProven,
    current_build_runtime_activation_proven: false,
    blocks_exact_current_build_relationship:
      hasHistoricalPacketEdge && !currentBuildRelationshipProven,
    historical_packet_build: String(scope.historical_packet_build ?? "unknown"),
    packet_family_edges: lineageEdges,
    static_evidence_counts: {
      definitions: (entry.tableDefinitions ?? []).length,
      table_incoming: (entry.tableIncomingReferences ?? []).length,
      client_assets: (entry.clientAssetReferences ?? []).length,
      unparsed_client_assets: (entry.unparsedClientAssetReferences ?? []).length,
      semantic_code_tokens: (entry.semanticCodeTokenReferences ?? []).length,
    },
  };
}

function loadRelationshipLineage(options, currentBuild) {
  const supplied = [options.currentBuffTable, options.baselineBuffTable, options.baselineBuild]
    .filter((value) => value !== undefined).length;
  if (supplied === 0) return null;
  assert(supplied === 3, "Relationship lineage requires --current-buff-table, --baseline-buff-table, and --baseline-build together");
  const currentPath = resolvePath(options.currentBuffTable);
  const baselinePath = resolvePath(options.baselineBuffTable);
  requireFile(currentPath);
  requireFile(baselinePath);
  const baselineBuild = String(options.baselineBuild);
  assert(baselineBuild !== currentBuild, "Relationship lineage baseline must differ from current build");
  return {
    currentPath,
    baselinePath,
    baselineBuild,
    currentRows: readJson(currentPath),
    baselineRows: readJson(baselinePath),
  };
}

function compareEdgeLineage(edge, lineage) {
  if (!lineage) return { ...edge, exact_parent_and_child_rows_unchanged: false };
  const parent = compareRow(lineage, edge.parent_effect_id);
  const child = compareRow(lineage, edge.child_effect_id);
  return {
    ...edge,
    relationship_lineage_baseline_build: lineage.baselineBuild,
    parent_row_lineage: parent,
    child_row_lineage: child,
    exact_parent_and_child_rows_unchanged: parent.unchanged === true && child.unchanged === true,
  };
}

function compareRow(lineage, effectId) {
  const key = String(effectId);
  const baseline = lineage.baselineRows[key];
  const current = lineage.currentRows[key];
  const baselineHash = baseline === undefined ? null : hashJson(baseline);
  const currentHash = current === undefined ? null : hashJson(current);
  return {
    effect_id: Number(effectId),
    baseline_sha256: baselineHash,
    current_sha256: currentHash,
    unchanged: baselineHash !== null && baselineHash === currentHash,
  };
}

function hashJson(value) {
  return createHash("sha256").update(JSON.stringify(stableValue(value))).digest("hex");
}

function stableValue(value) {
  if (Array.isArray(value)) return value.map(stableValue);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stableValue(value[key])]));
  }
  return value;
}

function collectPacketEdges(candidates) {
  const byChild = new Map();
  for (const candidate of candidates) {
    for (const edge of candidate.packet_observed_effect_family_edges ?? []) {
      const child = Number(edge.child_effect_id);
      if (!Number.isFinite(child)) continue;
      const normalized = {
        parent_effect_id: Number(edge.parent_effect_id),
        child_effect_id: child,
        source_type_id: edge.source_type_id == null ? null : Number(edge.source_type_id),
        observation_count: Number(edge.observation_count ?? 0),
        evidence_authority: edge.evidence_authority ?? null,
        source_ids: [String(candidate.source_id)],
      };
      const values = byChild.get(child) ?? [];
      const same = values.find((value) => edgeKey(value) === edgeKey(normalized));
      if (same) {
        if (!same.source_ids.includes(String(candidate.source_id))) {
          same.source_ids.push(String(candidate.source_id));
          same.source_ids.sort();
        }
        same.observation_count = Math.max(same.observation_count, normalized.observation_count);
      } else values.push(normalized);
      byChild.set(child, values);
    }
  }
  for (const values of byChild.values()) {
    values.sort((left, right) => edgeKey(left).localeCompare(edgeKey(right)));
  }
  return byChild;
}

function edgeKey(edge) {
  return [edge.parent_effect_id, edge.child_effect_id, edge.source_type_id, edge.evidence_authority].join(":");
}

function summarize(effects) {
  const statusCounts = Object.fromEntries(
    [...new Set(effects.map((entry) => entry.activation_status))]
      .sort()
      .map((status) => [status, effects.filter((entry) => entry.activation_status === status).length]),
  );
  return {
    effects: effects.length,
    packet_observed_family_children: effects.filter((entry) => entry.has_historical_packet_family_edge).length,
    definition_only_unobserved: effects.filter(
      (entry) => entry.activation_status === "definition-only-unobserved-in-indexed-packet-corpus",
    ).length,
    current_build_relationship_proven: effects.filter((entry) => entry.current_build_relationship_proven).length,
    relationship_lineage_carried_forward: effects.filter((entry) => entry.relationship_lineage_carried_forward).length,
    current_build_runtime_activation_proven: effects.filter((entry) => entry.current_build_runtime_activation_proven).length,
    current_build_relationship_blockers: effects.filter(
      (entry) => entry.blocks_exact_current_build_relationship,
    ).length,
    activation_status_counts: statusCounts,
  };
}

function selfTest() {
  const edges = collectPacketEdges([
    { source_id: "a", packet_observed_effect_family_edges: [{ parent_effect_id: 1, child_effect_id: 2, source_type_id: 1, observation_count: 5, evidence_authority: "historical" }] },
    { source_id: "b", packet_observed_effect_family_edges: [{ parent_effect_id: 1, child_effect_id: 2, source_type_id: 1, observation_count: 5, evidence_authority: "historical" }] },
  ]);
  assert(edges.get(2).length === 1, "Duplicate packet edges were not coalesced");
  assert(edges.get(2)[0].source_ids.join(",") === "a,b", "Packet edge source IDs were not retained");
  const stableHash = hashJson({ b: 2, a: { d: 4, c: 3 } });
  assert(stableHash === hashJson({ a: { c: 3, d: 4 }, b: 2 }), "Canonical row hashing is not stable");
  console.log("bpsr-effect-activation-ledger self-test passed");
}

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

function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function resolvePath(value) { return path.isAbsolute(value) ? value : path.resolve(repoRoot, value); }
function readJson(file) { return JSON.parse(readFileSync(file, "utf8")); }
function requireFile(file) { if (!existsSync(file)) throw new Error(`Missing required input ${file}`); }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function relativeRepo(file) { return path.relative(repoRoot, file).replaceAll("\\", "/"); }
function assert(condition, message) { if (!condition) throw new Error(message); }
function usage(exitCode) {
  console.log("Usage: node tools/bpsr-effect-activation-ledger.mjs generate --build-root <path> [--output <path>] [--current-buff-table <BuffTable.json> --baseline-buff-table <prior-BuffTable.json> --baseline-build <id>]");
  process.exit(exitCode);
}
