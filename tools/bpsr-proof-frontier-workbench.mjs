#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const DEFAULT_DEPTH = 5;
const PREVIEW_LIMIT = 48;
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")), options.index ? path.resolve(options.index) : null);
else if (command === "inspect") inspect(path.resolve(required(options, "input")), required(options, "source-rule"));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  const depth = Number(parsed.depth ?? DEFAULT_DEPTH);
  if (!Number.isInteger(depth) || depth < 1 || depth > 12) throw new Error("Depth must be an integer from 1 through 12");
  return {
    build: buildId,
    index: path.resolve(required(parsed, "index")),
    routes: path.resolve(required(parsed, "routes")),
    fieldAdjudications: path.resolve(required(parsed, "field-adjudications")),
    output: path.resolve(required(parsed, "output")),
    depth,
  };
}

function build(context) {
  const started = performance.now();
  requireFile(context.index, "semantic evidence index");
  requireFile(context.routes, "produced-damage proof routes");
  requireFile(context.fieldAdjudications, "semantic field adjudications");
  const routeLedger = readJson(context.routes, "produced-damage proof routes");
  if (String(routeLedger.game_build) !== context.build) throw new Error("Route ledger build mismatch");
  const adjudicationLedger = readJson(context.fieldAdjudications, "semantic field adjudications");
  if (String(adjudicationLedger.game_build) !== context.build) throw new Error("Field adjudication ledger build mismatch");
  const adjudications = new Map(
    adjudicationLedger.adjudications
      .filter((item) => item.proof_passed && item.disposition === "non-actionable-for-exact-output-routing")
      .map((item) => [item.semantic_field_key, item]),
  );
  const openRoutes = routeLedger.routes.filter((route) => !route.promotion_eligible);
  const db = new DatabaseSync(context.index, { readOnly: true });
  let report;
  try {
    const metadata = readMetadata(db);
    if (String(metadata.game_build) !== context.build) throw new Error("Evidence index build mismatch");
    const queries = createQueries(db);
    const routes = openRoutes.map((route) => compileRoute(route, queries, context.depth, adjudications));
    const shared = sharedFrontiers(routes);
    const sharedStructural = sharedStructuralFrontiers(routes);
    const sharedAdjudications = sharedAdjudicatedFields(routes);
    const exactStalls = routes.reduce((sum, route) => sum + route.exact_stalls.length, 0);
    const ambiguousBridges = routes.reduce((sum, route) => sum + route.ambiguous_bridges.length, 0);
    const adjudicatedOccurrences = routes.reduce((sum, route) => sum + route.adjudicated_non_actionable_fields.length, 0);
    const incomingIdCollisions = routes.reduce((sum, route) => sum + route.incoming_id_collisions.length, 0);
    report = {
      schema_version: 2,
      generated_by: "tools/bpsr-proof-frontier-workbench.mjs",
      game_build: context.build,
      policy: {
        derived_acceleration_only: true,
        never_promotes_relationships: true,
        exact_edges_remain_exact: true,
        ambiguous_occurrences_remain_quarantined: true,
        adjudicated_occurrences_remain_retained: true,
        adjudications_reduce_search_only: true,
        unresolved_routes_retained: true,
        zero_hidden_omissions: true,
      },
      inputs: {
        semantic_evidence_index: fileDescriptor(context.index),
        produced_damage_proof_routes: fileDescriptor(context.routes),
        semantic_field_adjudications: fileDescriptor(context.fieldAdjudications),
        evidence_index_source_fingerprint: metadata.source_fingerprint,
      },
      traversal: { maximum_exact_edge_depth: context.depth, row_preview_leaf_limit: PREVIEW_LIMIT },
      summary: {
        open_routes: routes.length,
        exact_stalls: exactStalls,
        ambiguous_bridges: ambiguousBridges,
        actionable_ambiguous_bridges: ambiguousBridges,
        adjudicated_non_actionable_occurrences: adjudicatedOccurrences,
        incoming_id_collisions: incomingIdCollisions,
        shared_frontiers: shared.length,
        shared_structural_frontiers: sharedStructural.length,
        shared_adjudicated_field_shapes: sharedAdjudications.length,
        quarantined_seed_occurrences: routes.reduce((sum, route) => sum + route.seed_occurrence_candidates.length, 0),
        routes_with_candidate_damage_targets: routes.filter((route) => route.candidate_damage_targets.length > 0).length,
        zero_hidden_omissions: openRoutes.length === routes.length,
      },
      shared_frontiers: shared,
      shared_structural_frontiers: sharedStructural,
      shared_adjudicated_field_shapes: sharedAdjudications,
      routes,
    };
    report.content_sha256 = contentHash(report);
  } finally {
    db.close();
  }
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`);
  verify(context.output, context.index);
  console.log(
    `Proof frontier workbench built for ${context.build}: ${report.summary.open_routes} open routes, ` +
    `${report.summary.exact_stalls} exact stalls, ${report.summary.ambiguous_bridges} outgoing fields to prove, ` +
    `${report.summary.incoming_id_collisions} incoming numeric collisions quarantined, ` +
    `${report.summary.shared_structural_frontiers} shared structural frontiers in ${Math.round(performance.now() - started)} ms.`,
  );
}

function sharedAdjudicatedFields(routes) {
  const groups = new Map();
  for (const route of routes) {
    for (const item of route.adjudicated_non_actionable_fields) {
      addShared(groups, `adjudicated:${item.semantic_field_key}:${item.semantic_role}`, "adjudicated-non-actionable-field", route, {
        semantic_field_key: item.semantic_field_key,
        semantic_role: item.semantic_role,
        disposition: item.disposition,
      });
    }
  }
  return [...groups.values()]
    .map((group) => ({ ...group, source_rule_ids: group.source_rule_ids.sort(compareText) }))
    .sort((left, right) => right.source_rule_ids.length - left.source_rule_ids.length || compareText(left.frontier_key, right.frontier_key));
}

function compileRoute(route, queries, maxDepth, adjudications) {
  const mechanicRow = queries.mechanic.get(route.source_rule_id);
  const mechanic = mechanicRow ? JSON.parse(mechanicRow.finding_json) : null;
  const starts = new Map();
  for (const row of mechanic?.decoded_rows ?? []) addNode(starts, row.table, row.row_id, "dependency-closure-row");
  const exactStartKeys = new Set([...starts.values()].map(nodeKey));
  const seedOccurrenceCandidates = (route.seed_ids ?? []).map((seed) => ({
    seed_id: String(seed),
    decoded_rows: queries.rowsById.all(String(seed)).map((row) => ({
      ...compactRow(row),
      already_in_exact_start_set: exactStartKeys.has(nodeKey({ table: row.table_name, id: String(row.row_id) })),
    })),
    lookup_command: `node tools/bpsr-semantic-evidence-index.mjs lookup --input <index> --id ${seed}`,
  }));

  const nodes = new Map();
  const edges = new Map();
  const queue = [...starts.values()].map((node) => ({ ...node, depth: 0 }));
  for (const node of queue) rememberNode(nodes, node, queries);
  const visitedDepth = new Map(queue.map((node) => [nodeKey(node), 0]));
  while (queue.length > 0) {
    const current = queue.shift();
    if (current.depth >= maxDepth) continue;
    for (const edge of queries.outgoing.all(current.table, current.id)) {
      const normalized = normalizeEdge(edge);
      edges.set(edgeKey(normalized), normalized);
      const next = { table: edge.target_table, id: String(edge.target_id), basis: "exact-edge-target", depth: current.depth + 1 };
      rememberNode(nodes, next, queries);
      const key = nodeKey(next);
      if (!visitedDepth.has(key) || next.depth < visitedDepth.get(key)) {
        visitedDepth.set(key, next.depth);
        queue.push(next);
      }
    }
  }

  const exactStalls = [];
  const ambiguous = new Map();
  const adjudicated = new Map();
  const incomingIdCollisions = new Map();
  for (const node of nodes.values()) {
    if (normalizeTable(node.table) !== "damageattrtable" && node.outgoing_exact_edge_count === 0) {
      exactStalls.push({
        frontier_key: `stall:${node.table}:${node.id}`,
        table: node.table,
        id: node.id,
        minimum_depth: node.minimum_depth,
        row_present: node.row_present,
        row_preview: node.row_preview,
        incoming_exact_edge_count: node.incoming_exact_edge_count,
        lookup_command: lookupCommand(node),
      });
    }
    for (const item of queries.ambiguousSource.all(node.table, node.id)) {
      const bridge = normalizeAmbiguous(item, node, "outgoing-frontier-field");
      const adjudication = adjudications.get(bridge.semantic_field_key);
      if (adjudication) {
        adjudicated.set(ambiguousKey(bridge), {
          ...bridge,
          proof_state: "retained-adjudicated-non-actionable-occurrence",
          semantic_role: adjudication.semantic_role,
          disposition: adjudication.disposition,
          acceleration_effect: adjudication.acceleration_effect,
        });
      } else ambiguous.set(ambiguousKey(bridge), bridge);
    }
    for (const item of queries.ambiguousTarget.all(node.id)) {
      const collision = normalizeAmbiguous(item, node, "incoming-numeric-collision");
      incomingIdCollisions.set(ambiguousKey(collision), collision);
    }
  }

  const candidateIds = [...new Set((route.candidate_routes ?? []).flatMap((candidate) => candidate.damage_ids ?? []).map(String))];
  const candidateTargets = candidateIds.map((id) => ({
    damage_id: id,
    decoded_rows: queries.rowsById.all(id).map(compactRow),
    incoming_exact_edges: queries.incomingById.all(id).map(normalizeEdge),
    ambiguous_occurrences: queries.ambiguousTarget.all(id).map((item) => normalizeAmbiguous(item, { table: "DamageAttrTable", id }, "candidate-damage-target-incoming")),
    lookup_command: `node tools/bpsr-semantic-evidence-index.mjs lookup --input <index> --id ${id} --table DamageAttrTable`,
  }));

  return {
    source_rule_id: route.source_rule_id,
    source_id: route.source_id,
    source_name: route.source_name,
    proof_state: route.proof_state,
    start_nodes: [...starts.values()].sort(compareNodes),
    seed_occurrence_candidates: seedOccurrenceCandidates,
    exact_neighborhood: {
      node_count: nodes.size,
      edge_count: edges.size,
      nodes: [...nodes.values()].sort(compareNodes),
      edges: [...edges.values()].sort(compareEdges),
    },
    exact_stalls: exactStalls.sort(compareFrontiers),
    ambiguous_bridges: [...ambiguous.values()].sort(compareAmbiguous),
    adjudicated_non_actionable_fields: [...adjudicated.values()].sort(compareAmbiguous),
    incoming_id_collisions: [...incomingIdCollisions.values()].sort(compareAmbiguous),
    candidate_damage_targets: candidateTargets,
    next_proof_action: route.next_proof_action,
    direct_inspect_command: `node tools/bpsr-proof-frontier-workbench.mjs inspect --input <workbench> --source-rule ${route.source_rule_id}`,
  };
}

function sharedStructuralFrontiers(routes) {
  const groups = new Map();
  for (const route of routes) {
    for (const stall of route.exact_stalls) {
      const pointers = (stall.row_preview ?? []).map((item) => item.pointer).sort(compareText);
      addShared(groups, `stall-shape:${stall.table}:${pointers.join(",")}`, "exact-stall-shape", route, {
        table: stall.table,
        row_preview_pointers: pointers,
      });
    }
    for (const bridge of route.ambiguous_bridges) {
      addShared(
        groups,
        `ambiguous-shape:${bridge.source_table}:${bridge.semantic_field_key}:${bridge.path_pattern}:${bridge.classification}`,
        "ambiguous-frontier-field-shape",
        route,
        {
          source_table: bridge.source_table,
          semantic_field_key: bridge.semantic_field_key,
          path_pattern: bridge.path_pattern,
          classification: bridge.classification,
        },
      );
    }
  }
  return [...groups.values()]
    .filter((group) => group.source_rule_ids.length > 1)
    .map((group) => ({ ...group, source_rule_ids: group.source_rule_ids.sort(compareText) }))
    .sort((left, right) => right.source_rule_ids.length - left.source_rule_ids.length || compareText(left.frontier_key, right.frontier_key));
}

function rememberNode(nodes, node, queries) {
  const key = nodeKey(node);
  const existing = nodes.get(key);
  if (existing) {
    existing.minimum_depth = Math.min(existing.minimum_depth, node.depth);
    if (!existing.bases.includes(node.basis)) existing.bases.push(node.basis);
    return;
  }
  const row = queries.rowByTableId.get(node.table, node.id);
  nodes.set(key, {
    table: node.table,
    id: String(node.id),
    minimum_depth: node.depth,
    bases: [node.basis],
    row_present: Boolean(row),
    row_sha256: row?.row_sha256 ?? null,
    row_preview: row ? previewRow(JSON.parse(row.row_json)) : [],
    outgoing_exact_edge_count: Number(queries.outgoingCount.get(node.table, node.id).count),
    incoming_exact_edge_count: Number(queries.incomingCount.get(node.table, node.id).count),
  });
}

function sharedFrontiers(routes) {
  const groups = new Map();
  for (const route of routes) {
    for (const stall of route.exact_stalls) addShared(groups, stall.frontier_key, "exact-stall", route, stall);
    for (const bridge of route.ambiguous_bridges) {
      const key = `ambiguous:${bridge.source_table}:${bridge.source_id}:${bridge.json_pointer}:${bridge.candidate_id}`;
      addShared(groups, key, "ambiguous-bridge", route, bridge);
    }
  }
  return [...groups.values()]
    .filter((group) => group.source_rule_ids.length > 1)
    .map((group) => ({ ...group, source_rule_ids: group.source_rule_ids.sort(compareText) }))
    .sort((left, right) => right.source_rule_ids.length - left.source_rule_ids.length || compareText(left.frontier_key, right.frontier_key));
}

function addShared(groups, key, kind, route, evidence) {
  if (!groups.has(key)) groups.set(key, { frontier_key: key, kind, source_rule_ids: [], evidence });
  const group = groups.get(key);
  if (!group.source_rule_ids.includes(route.source_rule_id)) group.source_rule_ids.push(route.source_rule_id);
}

function createQueries(db) {
  return {
    mechanic: db.prepare("SELECT finding_json FROM mechanic_findings WHERE source_rule_id=?"),
    rowsById: db.prepare("SELECT table_name,row_id,row_sha256,row_json FROM decoded_rows WHERE row_id=? ORDER BY table_name"),
    rowByTableId: db.prepare("SELECT table_name,row_id,row_sha256,row_json FROM decoded_rows WHERE table_name=? AND row_id=? LIMIT 1"),
    outgoing: db.prepare("SELECT source_table,source_id,source_field,source_pointer,relationship,target_table,target_id,proof FROM exact_edges WHERE source_table=? AND source_id=? ORDER BY target_table,target_id,source_pointer"),
    incomingById: db.prepare("SELECT source_table,source_id,source_field,source_pointer,relationship,target_table,target_id,proof FROM exact_edges WHERE target_id=? ORDER BY source_table,source_id,source_pointer"),
    outgoingCount: db.prepare("SELECT COUNT(*) count FROM exact_edges WHERE source_table=? AND source_id=?"),
    incomingCount: db.prepare("SELECT COUNT(*) count FROM exact_edges WHERE target_table=? AND target_id=?"),
    ambiguousTarget: db.prepare("SELECT source_table,source_id,field,path_pattern,semantic_field_key,json_pointer,candidate_id,classification FROM ambiguous_occurrences WHERE candidate_id=? ORDER BY source_table,source_id,json_pointer"),
    ambiguousSource: db.prepare("SELECT source_table,source_id,field,path_pattern,semantic_field_key,json_pointer,candidate_id,classification FROM ambiguous_occurrences WHERE source_table=? AND source_id=? ORDER BY json_pointer,candidate_id"),
  };
}

function verify(input, indexPath) {
  const report = readJson(input, "proof frontier workbench");
  if (report.schema_version !== 2) throw new Error("Proof frontier schema_version must be 2");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Proof frontier content hash mismatch");
  if (!report.policy?.never_promotes_relationships) throw new Error("Workbench must never promote relationships");
  if (!report.summary?.zero_hidden_omissions || report.summary.open_routes !== report.routes.length) throw new Error("Workbench hides open routes");
  const seen = new Set();
  let exactStalls = 0;
  let ambiguousBridges = 0;
  let adjudicatedOccurrences = 0;
  let incomingIdCollisions = 0;
  let seedOccurrences = 0;
  for (const route of report.routes) {
    if (seen.has(route.source_rule_id)) throw new Error(`Duplicate route ${route.source_rule_id}`);
    seen.add(route.source_rule_id);
    if (!route.next_proof_action) throw new Error(`Route ${route.source_rule_id} lacks its next proof action`);
    for (const field of ["exact_stalls", "ambiguous_bridges", "adjudicated_non_actionable_fields", "incoming_id_collisions", "seed_occurrence_candidates", "candidate_damage_targets"]) {
      if (!Array.isArray(route[field])) throw new Error(`Route ${route.source_rule_id} lacks ${field}`);
    }
    if (route.ambiguous_bridges.some((item) => item.evidence_direction !== "outgoing-frontier-field" || item.proof_state !== "quarantined-ambiguous-occurrence")) {
      throw new Error(`Route ${route.source_rule_id} contains a promoted or misdirected outgoing frontier`);
    }
    if (route.incoming_id_collisions.some((item) => item.evidence_direction !== "incoming-numeric-collision" || item.proof_state !== "quarantined-ambiguous-occurrence")) {
      throw new Error(`Route ${route.source_rule_id} contains an unquarantined incoming collision`);
    }
    if (route.adjudicated_non_actionable_fields.some((item) => item.evidence_direction !== "outgoing-frontier-field" || item.proof_state !== "retained-adjudicated-non-actionable-occurrence")) {
      throw new Error(`Route ${route.source_rule_id} contains a hidden or invalid adjudicated occurrence`);
    }
    const outgoingKeys = new Set(route.ambiguous_bridges.map(ambiguousKey));
    if (route.adjudicated_non_actionable_fields.some((item) => outgoingKeys.has(ambiguousKey(item)))) throw new Error(`Route ${route.source_rule_id} duplicates an outgoing occurrence`);
    exactStalls += route.exact_stalls.length;
    ambiguousBridges += route.ambiguous_bridges.length;
    adjudicatedOccurrences += route.adjudicated_non_actionable_fields.length;
    incomingIdCollisions += route.incoming_id_collisions.length;
    seedOccurrences += route.seed_occurrence_candidates.length;
  }
  if (report.summary.exact_stalls !== exactStalls) throw new Error("Exact stall summary mismatch");
  if (report.summary.ambiguous_bridges !== ambiguousBridges) throw new Error("Outgoing frontier summary mismatch");
  if (report.summary.actionable_ambiguous_bridges !== ambiguousBridges) throw new Error("Actionable frontier summary mismatch");
  if (report.summary.adjudicated_non_actionable_occurrences !== adjudicatedOccurrences) throw new Error("Adjudicated occurrence summary mismatch");
  if (report.summary.incoming_id_collisions !== incomingIdCollisions) throw new Error("Incoming collision summary mismatch");
  if (report.summary.quarantined_seed_occurrences !== seedOccurrences) throw new Error("Seed occurrence summary mismatch");
  if (!Array.isArray(report.shared_structural_frontiers)) throw new Error("Shared structural frontiers are missing");
  if (report.summary.shared_structural_frontiers !== report.shared_structural_frontiers.length) throw new Error("Shared structural frontier summary mismatch");
  if (!Array.isArray(report.shared_adjudicated_field_shapes) || report.summary.shared_adjudicated_field_shapes !== report.shared_adjudicated_field_shapes.length) throw new Error("Shared adjudicated field summary mismatch");
  if (indexPath) {
    const db = new DatabaseSync(indexPath, { readOnly: true });
    try { if (String(readMetadata(db).game_build) !== String(report.game_build)) throw new Error("Evidence index build mismatch"); }
    finally { db.close(); }
  }
  console.log(`Proof frontier workbench verified for build ${report.game_build}: ${report.routes.length} open routes, zero hidden omissions.`);
  return report;
}

function inspect(input, sourceRule) {
  const report = verify(input, null);
  const route = report.routes.find((item) => item.source_rule_id === sourceRule || item.source_id === sourceRule);
  if (!route) throw new Error(`Unknown source rule ${sourceRule}`);
  console.log(JSON.stringify(route, null, 2));
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-proof-frontier-"));
  try {
    const index = path.join(root, "index.sqlite");
    const db = new DatabaseSync(index);
    db.exec(`
      CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;
      CREATE TABLE decoded_rows(table_name TEXT,storage_key TEXT,row_id TEXT,row_sha256 TEXT,row_json TEXT);
      CREATE TABLE exact_edges(source_table TEXT,source_id TEXT,source_field TEXT,source_pointer TEXT,relationship TEXT,target_table TEXT,target_id TEXT,proof TEXT,edge_json TEXT);
      CREATE TABLE ambiguous_occurrences(source_table TEXT,source_id TEXT,field TEXT,path_pattern TEXT,semantic_field_key TEXT,json_pointer TEXT,candidate_id TEXT,classification TEXT);
      CREATE TABLE mechanic_findings(source_id TEXT,source_name TEXT,source_rule_id TEXT,source_kind TEXT,source_type TEXT,source_table TEXT,promotion_blocked INTEGER,issue_categories_json TEXT,finding_json TEXT);
      INSERT INTO metadata VALUES('game_build','1'),('source_fingerprint','test');
      INSERT INTO decoded_rows VALUES('BuffTable','10','10','a','{"Id":10,"SkillId":20}');
      INSERT INTO decoded_rows VALUES('SkillTable','20','20','b','{"Id":20,"DamageId":900}');
      INSERT INTO exact_edges VALUES('BuffTable','10','SkillId','/SkillId','exact-reference','SkillTable','20','exact-field','{}');
      INSERT INTO ambiguous_occurrences VALUES('SkillTable','20','DamageId','/DamageId','k','/DamageId','900','ambiguous-namespace');
      INSERT INTO ambiguous_occurrences VALUES('SkillTable','20','EntryDatabaseId','/EntryDatabaseId','entry-database','/EntryDatabaseId','1','ambiguous-namespace');
      INSERT INTO ambiguous_occurrences VALUES('HousingItems','77','SortId','/SortId','housing-sort','/SortId','20','ambiguous-namespace');
      INSERT INTO mechanic_findings VALUES('source:10','Test','mrs:test','buff',NULL,'BuffTable',1,'[]','{"decoded_rows":[{"table":"BuffTable","row_id":"10"}]}');
    `);
    db.close();
    const routesFile = path.join(root, "routes.json");
    writeFileSync(routesFile, JSON.stringify({ game_build: "1", routes: [{ source_rule_id: "mrs:test", source_id: "source:10", source_name: "Test", proof_state: "no-candidate", promotion_eligible: false, seed_ids: ["10"], candidate_routes: [], next_proof_action: "Prove 20 to 900." }] }));
    const output = path.join(root, "workbench.json");
    const adjudicationsFile = path.join(root, "adjudications.json");
    writeFileSync(adjudicationsFile, JSON.stringify({ game_build: "1", adjudications: [{ semantic_field_key: "entry-database", semantic_role: "scene-membership", disposition: "non-actionable-for-exact-output-routing", proof_passed: true, acceleration_effect: "test" }] }));
    build({ build: "1", index, routes: routesFile, fieldAdjudications: adjudicationsFile, output, depth: 3 });
    const report = readJson(output, "self-test output");
    if (report.routes[0].exact_stalls[0]?.id !== "20") throw new Error("Exact stall not retained");
    if (report.routes[0].ambiguous_bridges[0]?.candidate_id !== "900") throw new Error("Ambiguous bridge not retained");
    if (report.routes[0].adjudicated_non_actionable_fields[0]?.candidate_id !== "1") throw new Error("Adjudicated occurrence not retained separately");
    if (report.routes[0].incoming_id_collisions[0]?.source_table !== "HousingItems") throw new Error("Incoming numeric collision not separated");
    console.log("Proof frontier workbench self-test passed: exact stalls, actionable fields, adjudicated fields, and incoming numeric collisions remain distinct and reviewable.");
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function previewRow(value) {
  const leaves = [];
  walk(value, "", (pointer, key, item) => {
    if (leaves.length >= PREVIEW_LIMIT || !isPrimitive(item)) return;
    if (/id|damage|buff|skill|bullet|summon|entity|base|effect|entrybd|talent/i.test(key)) {
      leaves.push({ pointer, value: typeof item === "string" && item.length > 240 ? `${item.slice(0, 237)}...` : item });
    }
  });
  return leaves;
}

function walk(value, pointer, visitor) {
  if (!value || typeof value !== "object") return;
  for (const [key, item] of Object.entries(value)) {
    const next = `${pointer}/${escapePointer(key)}`;
    visitor(next, key, item);
    if (item && typeof item === "object") walk(item, next, visitor);
  }
}

function normalizeAmbiguous(item, node, evidenceDirection) {
  return { ...item, candidate_id: String(item.candidate_id), evidence_direction: evidenceDirection, reached_from: { table: node.table, id: String(node.id) }, proof_state: "quarantined-ambiguous-occurrence" };
}
function normalizeEdge(edge) { return { ...edge, source_id: String(edge.source_id), target_id: String(edge.target_id) }; }
function compactRow(row) { return { table: row.table_name, id: String(row.row_id), row_sha256: row.row_sha256, row_preview: previewRow(JSON.parse(row.row_json)) }; }
function addNode(map, table, id, basis) { if (table && id !== undefined) map.set(`${table}\0${id}`, { table, id: String(id), basis, depth: 0 }); }
function nodeKey(node) { return `${node.table}\0${node.id}`; }
function edgeKey(edge) { return `${edge.source_table}\0${edge.source_id}\0${edge.source_pointer}\0${edge.target_table}\0${edge.target_id}`; }
function ambiguousKey(item) { return `${item.source_table}\0${item.source_id}\0${item.json_pointer}\0${item.candidate_id}`; }
function lookupCommand(node) { return `node tools/bpsr-semantic-evidence-index.mjs lookup --input <index> --id ${node.id} --table ${node.table}`; }
function normalizeTable(value) { return String(value ?? "").replaceAll(/[^a-z0-9]/gi, "").toLowerCase(); }
function compareNodes(a, b) { return a.minimum_depth - b.minimum_depth || compareText(a.table, b.table) || compareIdentifiers(a.id, b.id); }
function compareEdges(a, b) { return compareText(a.source_table, b.source_table) || compareIdentifiers(a.source_id, b.source_id) || compareText(a.source_pointer, b.source_pointer) || compareText(a.target_table, b.target_table) || compareIdentifiers(a.target_id, b.target_id); }
function compareFrontiers(a, b) { return a.minimum_depth - b.minimum_depth || compareText(a.frontier_key, b.frontier_key); }
function compareAmbiguous(a, b) { return compareText(a.source_table, b.source_table) || compareIdentifiers(a.source_id, b.source_id) || compareText(a.json_pointer, b.json_pointer) || compareIdentifiers(a.candidate_id, b.candidate_id); }
function compareText(a, b) { return String(a ?? "").localeCompare(String(b ?? ""), "en", { numeric: true }); }
function compareIdentifiers(a, b) { try { const x = BigInt(a); const y = BigInt(b); return x < y ? -1 : x > y ? 1 : 0; } catch { return compareText(a, b); } }
function isPrimitive(value) { return value === null || ["string", "number", "boolean"].includes(typeof value); }
function escapePointer(value) { return String(value).replaceAll("~", "~0").replaceAll("/", "~1"); }
function readMetadata(db) { return Object.fromEntries(db.prepare("SELECT key,value FROM metadata ORDER BY key").all().map((row) => [row.key, row.value])); }
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function fileDescriptor(file) { const data = readFileSync(file); return { path: file.replaceAll("\\", "/"), bytes: data.length, sha256: createHash("sha256").update(data).digest("hex") }; }
function requireFile(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function parseArgs(args) { const parsed = {}; for (let i = 0; i < args.length; i += 2) { const key = args[i]; const value = args[i + 1]; if (!key?.startsWith("--") || value === undefined || value.startsWith("--")) throw new Error(`Invalid argument near ${key}`); parsed[key.slice(2)] = value; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log(`Usage:\n  node tools/bpsr-proof-frontier-workbench.mjs build --build <id> --index <sqlite> --routes <json> --field-adjudications <json> --output <json> [--depth 5]\n  node tools/bpsr-proof-frontier-workbench.mjs verify --input <json> [--index <sqlite>]\n  node tools/bpsr-proof-frontier-workbench.mjs inspect --input <json> --source-rule <id>\n  node tools/bpsr-proof-frontier-workbench.mjs self-test`); process.exit(exitCode); }
