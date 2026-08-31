#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveBuildContext(options));
else if (command === "verify") verify(
  path.resolve(required(options, "input")),
  options["verify-sources"] !== "false",
);
else if (command === "lookup") lookup(options);
else if (command === "mechanic") mechanic(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveBuildContext(options) {
  const build = required(options, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  return {
    build,
    decodedRoot: path.resolve(required(options, "decoded-root")),
    referenceGraph: path.resolve(required(options, "reference-graph")),
    referenceOccurrences: path.resolve(required(options, "reference-occurrences")),
    referenceCandidates: path.resolve(required(options, "reference-candidates")),
    expressionEdges: path.resolve(required(options, "expression-edges")),
    semanticClosure: path.resolve(required(options, "semantic-closure")),
    semanticAudit: path.resolve(required(options, "semantic-audit")),
    output: path.resolve(required(options, "output")),
  };
}

function build(context) {
  const started = performance.now();
  const graph = readJson(context.referenceGraph, "reference graph");
  const expressionEdges = readJson(context.expressionEdges, "decoded expression reference edges");
  const closure = readJson(context.semanticClosure, "semantic dependency closure");
  const audit = readJson(context.semanticAudit, "semantic audit");
  for (const [label, value] of [
    ["reference graph", graph],
    ["decoded expression reference edges", expressionEdges],
    ["semantic dependency closure", closure],
    ["semantic audit", audit],
  ]) requireBuild(value, context.build, label);

  const decodedFiles = graph.tables.map((table) => ({
    label: `decoded:${table.table}`,
    path: path.join(context.decodedRoot, table.file),
    table: table.table,
    expectedRows: table.rows,
    rowHashes: table.row_hashes,
  }));
  const sources = [
    { label: "reference-graph", path: context.referenceGraph },
    { label: "reference-occurrences", path: context.referenceOccurrences },
    { label: "reference-candidates", path: context.referenceCandidates },
    { label: "decoded-expression-reference-edges", path: context.expressionEdges },
    { label: "semantic-closure", path: context.semanticClosure },
    { label: "semantic-audit", path: context.semanticAudit },
    ...decodedFiles,
  ];
  for (const source of sources) requireFile(source.path, source.label);

  mkdirSync(path.dirname(context.output), { recursive: true });
  const temporary = `${context.output}.building-${process.pid}`;
  rmSync(temporary, { force: true });
  const db = new DatabaseSync(temporary);
  try {
    db.exec(`
      PRAGMA journal_mode = OFF;
      PRAGMA synchronous = OFF;
      PRAGMA temp_store = MEMORY;
      PRAGMA locking_mode = EXCLUSIVE;
      CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
      CREATE TABLE source_files (
        label TEXT PRIMARY KEY, path TEXT NOT NULL, bytes INTEGER NOT NULL, sha256 TEXT NOT NULL
      ) WITHOUT ROWID;
      CREATE TABLE decoded_rows (
        table_name TEXT NOT NULL, storage_key TEXT NOT NULL, row_id TEXT NOT NULL,
        row_sha256 TEXT NOT NULL, row_json TEXT NOT NULL,
        PRIMARY KEY (table_name, storage_key)
      ) WITHOUT ROWID;
      CREATE TABLE exact_edges (
        source_table TEXT NOT NULL, source_id TEXT NOT NULL, source_field TEXT NOT NULL,
        source_pointer TEXT NOT NULL, relationship TEXT NOT NULL, target_table TEXT NOT NULL,
        target_id TEXT NOT NULL, proof TEXT NOT NULL, edge_json TEXT NOT NULL
      );
      CREATE TABLE ambiguous_occurrences (
        source_table TEXT NOT NULL, source_id TEXT NOT NULL, field TEXT NOT NULL,
        path_pattern TEXT NOT NULL, semantic_field_key TEXT NOT NULL, json_pointer TEXT NOT NULL,
        candidate_id TEXT NOT NULL, classification TEXT NOT NULL
      );
      CREATE TABLE reference_candidates (
        semantic_field_key TEXT PRIMARY KEY, source_table TEXT NOT NULL, field TEXT NOT NULL,
        path_pattern TEXT NOT NULL, candidate_json TEXT NOT NULL
      ) WITHOUT ROWID;
      CREATE TABLE semantic_findings (
        source_id TEXT PRIMARY KEY, source_rule_id TEXT NOT NULL, source_name TEXT NOT NULL,
        promotion_blocked INTEGER NOT NULL, finding_json TEXT NOT NULL
      ) WITHOUT ROWID;
      CREATE TABLE mechanic_findings (
        source_id TEXT PRIMARY KEY, source_name TEXT NOT NULL, source_rule_id TEXT NOT NULL,
        source_kind TEXT, source_type TEXT, source_table TEXT, promotion_blocked INTEGER NOT NULL,
        issue_categories_json TEXT NOT NULL, finding_json TEXT NOT NULL
      ) WITHOUT ROWID;
      CREATE TABLE mechanic_rows (
        source_id TEXT NOT NULL, table_name TEXT NOT NULL, row_id TEXT NOT NULL, depth INTEGER NOT NULL,
        reached_via TEXT NOT NULL, row_sha256 TEXT NOT NULL,
        PRIMARY KEY (source_id, table_name, row_id)
      ) WITHOUT ROWID;
      CREATE TABLE mechanic_fields (
        source_id TEXT NOT NULL, table_name TEXT NOT NULL, row_id TEXT NOT NULL,
        pointer TEXT NOT NULL, normalized_pointer TEXT NOT NULL, value_json TEXT NOT NULL,
        field_schema_state TEXT NOT NULL
      );
      CREATE TABLE mechanic_dependencies (
        source_id TEXT NOT NULL, kind TEXT NOT NULL, severity TEXT,
        required_model TEXT, evidence TEXT, dependency_json TEXT NOT NULL
      );
      CREATE TABLE mechanic_external_references (
        source_id TEXT NOT NULL, kind TEXT NOT NULL, seed_id TEXT NOT NULL,
        status TEXT NOT NULL, authority TEXT NOT NULL, reference_json TEXT NOT NULL
      );
    `);

    const sourceInsert = db.prepare(
      "INSERT INTO source_files(label,path,bytes,sha256) VALUES(?,?,?,?)",
    );
    db.exec("BEGIN");
    for (const source of sources) {
      const stats = statSync(source.path);
      sourceInsert.run(source.label, normalizePath(source.path), stats.size, hashFile(source.path));
    }
    db.exec("COMMIT");

    const rowInsert = db.prepare(
      "INSERT INTO decoded_rows(table_name,storage_key,row_id,row_sha256,row_json) VALUES(?,?,?,?,?)",
    );
    let decodedRowCount = 0;
    for (const source of decodedFiles) {
      const rows = readJson(source.path, source.label);
      const entries = normalizeRows(rows);
      if (entries.length !== source.expectedRows) {
        throw new Error(`${source.label} row mismatch: ${entries.length} != ${source.expectedRows}`);
      }
      db.exec("BEGIN");
      for (const entry of entries) {
        const rowHash = hashText(stableStringify(entry.value));
        rowInsert.run(
          source.table, entry.storageKey, entry.rowId, rowHash, JSON.stringify(entry.value),
        );
        decodedRowCount += 1;
      }
      db.exec("COMMIT");
    }

    const edgeInsert = db.prepare(
      "INSERT INTO exact_edges VALUES(?,?,?,?,?,?,?,?,?)",
    );
    const exactEdges = deduplicateExactEdges(expandResolvedExactEdges([
      ...graph.exact_edges,
      ...expressionEdges.exact_edges,
    ]));
    db.exec("BEGIN");
    for (const edge of exactEdges) {
      edgeInsert.run(
        edge.source_table, String(edge.source_id), edge.source_field, edge.source_pointer,
        edge.relationship,
        edge.target_table,
        String(edge.target_id), edge.proof, JSON.stringify(edge),
      );
    }
    db.exec("COMMIT");

    const occurrenceInsert = db.prepare(
      "INSERT INTO ambiguous_occurrences VALUES(?,?,?,?,?,?,?,?)",
    );
    let occurrenceCount = 0;
    db.exec("BEGIN");
    forEachJsonLine(context.referenceOccurrences, (row) => {
      occurrenceInsert.run(
        row.source_table, String(row.source_id), row.field, row.path_pattern,
        row.semantic_field_key, row.json_pointer, String(row.candidate_id), row.classification,
      );
      occurrenceCount += 1;
    });
    db.exec("COMMIT");

    const candidateInsert = db.prepare(
      "INSERT INTO reference_candidates VALUES(?,?,?,?,?)",
    );
    let candidateCount = 0;
    db.exec("BEGIN");
    forEachJsonLine(context.referenceCandidates, (row) => {
      candidateInsert.run(
        row.semantic_field_key, row.source_table, row.field, row.path_pattern, JSON.stringify(row),
      );
      candidateCount += 1;
    });
    db.exec("COMMIT");

    const semanticInsert = db.prepare("INSERT INTO semantic_findings VALUES(?,?,?,?,?)");
    db.exec("BEGIN");
    for (const finding of audit.findings) {
      semanticInsert.run(
        finding.source_id, finding.source_rule_id, finding.source_name,
        finding.promotion_blocked ? 1 : 0, JSON.stringify(finding),
      );
    }
    db.exec("COMMIT");

    const mechanicInsert = db.prepare("INSERT INTO mechanic_findings VALUES(?,?,?,?,?,?,?,?,?)");
    const mechanicRowInsert = db.prepare("INSERT INTO mechanic_rows VALUES(?,?,?,?,?,?)");
    const mechanicFieldInsert = db.prepare("INSERT INTO mechanic_fields VALUES(?,?,?,?,?,?,?)");
    const dependencyInsert = db.prepare("INSERT INTO mechanic_dependencies VALUES(?,?,?,?,?,?)");
    const externalReferenceInsert = db.prepare(
      "INSERT INTO mechanic_external_references VALUES(?,?,?,?,?,?)",
    );
    let mechanicRowCount = 0;
    let mechanicFieldCount = 0;
    let mechanicDependencyCount = 0;
    let mechanicExternalReferenceCount = 0;
    db.exec("BEGIN");
    for (const finding of closure.mechanics) {
      mechanicInsert.run(
        finding.source_id, finding.source_name, finding.source_rule_id,
        finding.source_kind ?? null, finding.source_type ?? null, finding.source_table ?? null,
        finding.promotion_blocked ? 1 : 0, JSON.stringify(finding.issue_categories),
        JSON.stringify(finding),
      );
      for (const row of finding.decoded_rows) {
        mechanicRowInsert.run(
          finding.source_id, row.table, String(row.row_id), row.depth, row.reached_via,
          row.row_sha256,
        );
        mechanicRowCount += 1;
      }
      for (const field of finding.mechanics_sensitive_fields) {
        mechanicFieldInsert.run(
          finding.source_id, field.table, String(field.row_id), field.pointer,
          field.normalized_pointer, JSON.stringify(field.value), field.field_schema_state,
        );
        mechanicFieldCount += 1;
      }
      for (const dependency of finding.unresolved_dependencies) {
        dependencyInsert.run(
          finding.source_id, dependency.kind, dependency.severity ?? null,
          dependency.required_model ?? null, dependency.evidence ?? null,
          JSON.stringify(dependency),
        );
        mechanicDependencyCount += 1;
      }
      for (const reference of finding.external_references ?? []) {
        externalReferenceInsert.run(
          finding.source_id, reference.kind, String(reference.seed_id), reference.status,
          reference.authority, JSON.stringify(reference),
        );
        mechanicExternalReferenceCount += 1;
      }
    }
    db.exec("COMMIT");

    db.exec(`
      CREATE INDEX exact_edges_source ON exact_edges(source_table, source_id);
      CREATE INDEX exact_edges_target ON exact_edges(target_table, target_id);
      CREATE INDEX decoded_rows_id ON decoded_rows(row_id, table_name);
      CREATE INDEX ambiguous_candidate ON ambiguous_occurrences(candidate_id);
      CREATE INDEX ambiguous_source ON ambiguous_occurrences(source_table, source_id);
      CREATE INDEX mechanic_rows_lookup ON mechanic_rows(table_name, row_id);
      CREATE INDEX mechanic_fields_lookup ON mechanic_fields(table_name, row_id, normalized_pointer);
      CREATE INDEX mechanic_dependencies_kind ON mechanic_dependencies(kind, source_id);
      CREATE INDEX mechanic_external_references_seed ON mechanic_external_references(seed_id, source_id);
      CREATE INDEX mechanic_external_references_source ON mechanic_external_references(source_id, kind);
      ANALYZE;
    `);

    const counts = {
      decoded_rows: decodedRowCount,
      exact_edges: exactEdges.length,
      ambiguous_occurrences: occurrenceCount,
      reference_candidates: candidateCount,
      semantic_findings: audit.findings.length,
      mechanic_findings: closure.mechanics.length,
      mechanic_rows: mechanicRowCount,
      mechanic_fields: mechanicFieldCount,
      mechanic_dependencies: mechanicDependencyCount,
      mechanic_external_references: mechanicExternalReferenceCount,
    };
    const sourceFingerprint = hashText(JSON.stringify(
      db.prepare("SELECT label,bytes,sha256 FROM source_files ORDER BY label").all(),
    ));
    const meta = {
      schema_version: 1,
      generated_by: "tools/bpsr-semantic-evidence-index.mjs",
      game_build: context.build,
      source_fingerprint: sourceFingerprint,
      decoded_expression_exact_edges: expressionEdges.exact_edges.length,
      counts,
      policy: {
        index_is_acceleration_not_authority: true,
        source_hashes_preserved: true,
        unresolved_evidence_retained: true,
        identifier_values_rewritten: false,
        packet_proof_requirements_unchanged: true,
        typed_expression_edges_build_locked: true,
      },
    };
    const metadataInsert = db.prepare("INSERT INTO metadata VALUES(?,?)");
    db.exec("BEGIN");
    for (const [key, value] of Object.entries(meta)) {
      metadataInsert.run(key, typeof value === "string" ? value : JSON.stringify(value));
    }
    db.exec("COMMIT");
    db.exec("PRAGMA optimize");
  } finally {
    db.close();
  }

  if (existsSync(context.output)) rmSync(context.output, { force: true });
  renameSync(temporary, context.output);
  const result = verify(context.output, false);
  console.log(
    `Semantic evidence index built for ${context.build}: ${result.counts.decoded_rows} decoded rows, ` +
    `${result.counts.exact_edges} exact edges, ${result.counts.ambiguous_occurrences} retained ambiguous occurrences ` +
    `in ${Math.round(performance.now() - started)} ms.`,
  );
}

function verify(input, verifySources) {
  requireFile(input, "semantic evidence index");
  const db = new DatabaseSync(input, { readOnly: true });
  try {
    const quickCheck = db.prepare("PRAGMA quick_check").get();
    if (quickCheck.quick_check !== "ok") throw new Error(`SQLite quick_check failed: ${quickCheck.quick_check}`);
    const metadata = Object.fromEntries(
      db.prepare("SELECT key,value FROM metadata ORDER BY key").all().map((row) => [row.key, row.value]),
    );
    if (Number(metadata.schema_version) !== 1) throw new Error("Evidence index schema_version must be 1");
    const counts = JSON.parse(metadata.counts);
    for (const [table, expected] of Object.entries(counts)) {
      const actual = Number(db.prepare(`SELECT COUNT(*) AS count FROM ${safeTable(table)}`).get().count);
      if (actual !== expected) throw new Error(`${table} count mismatch: ${actual} != ${expected}`);
    }
    if (verifySources) {
      for (const source of db.prepare("SELECT * FROM source_files ORDER BY label").all()) {
        requireFile(source.path, source.label);
        const stats = statSync(source.path);
        if (stats.size !== source.bytes) throw new Error(`${source.label} size changed`);
        if (hashFile(source.path) !== source.sha256) throw new Error(`${source.label} hash changed`);
      }
    }
    const result = { game_build: metadata.game_build, counts };
    console.log(`Semantic evidence index verified for build ${result.game_build}: zero hidden omissions.`);
    return result;
  } finally {
    db.close();
  }
}

function lookup(options) {
  const input = path.resolve(required(options, "input"));
  const id = required(options, "id");
  const table = options.table ?? null;
  const db = new DatabaseSync(input, { readOnly: true });
  try {
    const decoded = table
      ? db.prepare("SELECT * FROM decoded_rows WHERE table_name=? AND (row_id=? OR storage_key=?)").all(table, id, id)
      : db.prepare("SELECT * FROM decoded_rows WHERE row_id=? ORDER BY table_name").all(id);
    const result = {
      id,
      decoded_rows: decoded.map((row) => ({ ...row, row: JSON.parse(row.row_json), row_json: undefined })),
      outgoing_exact_edges: db.prepare(
        "SELECT * FROM exact_edges WHERE source_id=? ORDER BY source_table,source_field,target_table,target_id",
      ).all(id),
      incoming_exact_edges: db.prepare(
        "SELECT * FROM exact_edges WHERE target_id=? ORDER BY target_table,source_table,source_id",
      ).all(id),
      ambiguous_occurrences: db.prepare(
        "SELECT * FROM ambiguous_occurrences WHERE candidate_id=? OR source_id=? ORDER BY source_table,source_id,json_pointer",
      ).all(id, id),
      mechanic_membership: db.prepare(
        "SELECT * FROM mechanic_rows WHERE row_id=? ORDER BY source_id,table_name",
      ).all(id),
      external_reference_membership: db.prepare(
        "SELECT * FROM mechanic_external_references WHERE seed_id=? ORDER BY source_id,kind",
      ).all(id),
    };
    console.log(JSON.stringify(result, null, 2));
  } finally {
    db.close();
  }
}

function mechanic(options) {
  const input = path.resolve(required(options, "input"));
  const sourceId = required(options, "source-id");
  const db = new DatabaseSync(input, { readOnly: true });
  try {
    const finding = db.prepare("SELECT * FROM mechanic_findings WHERE source_id=?").get(sourceId);
    if (!finding) throw new Error(`Unknown mechanic source ${sourceId}`);
    console.log(JSON.stringify({
      finding: JSON.parse(finding.finding_json),
      semantic_finding: parseJsonColumn(
        db.prepare("SELECT finding_json FROM semantic_findings WHERE source_id=?").get(sourceId),
        "finding_json",
      ),
      decoded_rows: db.prepare("SELECT * FROM mechanic_rows WHERE source_id=? ORDER BY depth,table_name,row_id").all(sourceId),
      fields: db.prepare("SELECT * FROM mechanic_fields WHERE source_id=? ORDER BY table_name,row_id,normalized_pointer").all(sourceId),
      unresolved_dependencies: db.prepare("SELECT * FROM mechanic_dependencies WHERE source_id=? ORDER BY kind").all(sourceId),
      external_references: db.prepare(
        "SELECT * FROM mechanic_external_references WHERE source_id=? ORDER BY kind,seed_id",
      ).all(sourceId),
    }, null, 2));
  } finally {
    db.close();
  }
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-semantic-index-test-"));
  try {
    const decodedRoot = path.join(root, "decoded");
    mkdirSync(decodedRoot);
    writeJson(path.join(decodedRoot, "BuffTable.json"), { "10": { Id: 10, SkillId: 20, Value: 5 } });
    const graph = {
      game_build: "1",
      tables: [{ table: "BuffTable", file: "BuffTable.json", rows: 1, row_hashes: { "10": hashText(JSON.stringify({ Id: 10, SkillId: 20, Value: 5 })) } }],
      exact_edges: [
        { source_table: "BuffTable", source_id: "10", source_field: "SkillId", source_pointer: "/SkillId", relationship: "buff-owner-skill", target_table: "SkillTable", target_id: "20", proof: "test" },
        {
          source_table: "BuffTable", source_id: "10", source_field: "EffectIDs",
          source_pointer: "/EffectIDs/0", relationship: "test-multi-table-reference",
          target_id: "40", proof: "test-resolved-target-tables",
          target_tables: ["SkillEffectTable", "SkillFightLevelTable"],
          resolved_target_tables: ["SkillEffectTable", "SkillFightLevelTable"],
        },
      ],
    };
    const closure = {
      game_build: "1",
      mechanics: [{
        source_id: "buff-source:10", source_name: "Test", source_rule_id: "test:1",
        source_kind: "test", source_type: "test", source_table: "BuffTable",
        promotion_blocked: true, issue_categories: ["formula-magnitude-unresolved"],
        decoded_rows: [{ table: "BuffTable", row_id: "10", depth: 0, reached_via: "seed", row_sha256: graph.tables[0].row_hashes["10"] }],
        mechanics_sensitive_fields: [{ table: "BuffTable", row_id: "10", pointer: "/Value", normalized_pointer: "/Value", value: 5, field_schema_state: "inventoried" }],
        external_references: [{ kind: "localization-key", seed_id: "99", status: "retained-outside-decoded-table-namespace", authority: "game-locale-assets-via-localization-plugin" }],
        unresolved_dependencies: [{ kind: "formula-magnitude-unresolved", severity: "error", required_model: "exact-value", evidence: "test" }],
      }],
    };
    const audit = { game_build: "1", findings: [{ source_id: "buff-source:10", source_rule_id: "test:1", source_name: "Test", promotion_blocked: true }] };
    const graphPath = path.join(root, "graph.json");
    const closurePath = path.join(root, "closure.json");
    const auditPath = path.join(root, "audit.json");
    const occurrencesPath = path.join(root, "occurrences.jsonl");
    const candidatesPath = path.join(root, "candidates.jsonl");
    const expressionEdgesPath = path.join(root, "expression-edges.json");
    const output = path.join(root, "index.sqlite");
    writeJson(graphPath, graph);
    writeJson(closurePath, closure);
    writeJson(auditPath, audit);
    writeJson(expressionEdgesPath, {
      game_build: "1",
      exact_edges: [{
        source_table: "BuffTable", source_id: "10", source_field: "Value",
        source_pointer: "/Value", relationship: "test-expression-reference",
        target_table: "DamageAttrTable", target_id: "30", proof: "test-expression",
      }],
    });
    writeFileSync(occurrencesPath, JSON.stringify({ source_table: "BuffTable", source_id: "10", field: "SkillId", path_pattern: "/SkillId", semantic_field_key: "BuffTable/SkillId", json_pointer: "/SkillId", candidate_id: "20", classification: "reference-like-unproven" }) + "\n");
    writeFileSync(candidatesPath, JSON.stringify({ semantic_field_key: "BuffTable/SkillId", source_table: "BuffTable", field: "SkillId", path_pattern: "/SkillId" }) + "\n");
    build({ build: "1", decodedRoot, referenceGraph: graphPath, referenceOccurrences: occurrencesPath, referenceCandidates: candidatesPath, expressionEdges: expressionEdgesPath, semanticClosure: closurePath, semanticAudit: auditPath, output });
    const verified = verify(output, true);
    if (verified.counts.decoded_rows !== 1
      || verified.counts.exact_edges !== 4
      || verified.counts.mechanic_findings !== 1
      || verified.counts.mechanic_external_references !== 1) {
      throw new Error("Self-test conservation failed");
    }
    const db = new DatabaseSync(output, { readOnly: true });
    const row = db.prepare("SELECT row_json FROM decoded_rows WHERE table_name='BuffTable' AND row_id='10'").get();
    const externalReference = db.prepare(
      "SELECT seed_id FROM mechanic_external_references WHERE source_id='buff-source:10'",
    ).get();
    const expandedTargets = db.prepare(
      "SELECT target_table FROM exact_edges WHERE target_id='40' ORDER BY target_table",
    ).all().map((edge) => edge.target_table);
    db.close();
    if (JSON.parse(row.row_json).Value !== 5) throw new Error("Self-test lookup failed");
    if (externalReference.seed_id !== "99") throw new Error("Self-test external reference lookup failed");
    if (expandedTargets.join(",") !== "SkillEffectTable,SkillFightLevelTable") {
      throw new Error("Self-test resolved multi-table edge expansion failed");
    }
    console.log("bpsr-semantic-evidence-index self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function forEachJsonLine(file, callback) {
  const text = readFileSync(file, "utf8");
  for (const line of text.split(/\r?\n/)) {
    if (line.trim()) callback(JSON.parse(line));
  }
}

function deduplicateExactEdges(edges) {
  const seen = new Set();
  const result = [];
  for (const edge of edges) {
    const targetTable = edge.target_table;
    if (!targetTable) {
      throw new Error(
        `Exact edge is missing a resolved target table: ${edge.source_table}:${edge.source_id}${edge.source_pointer}`,
      );
    }
    const key = [
      edge.source_table, String(edge.source_id), edge.source_field, edge.source_pointer,
      edge.relationship, targetTable, String(edge.target_id), edge.proof,
    ].join("\u001f");
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(edge);
  }
  return result;
}

function expandResolvedExactEdges(edges) {
  const result = [];
  for (const edge of edges) {
    if (edge.target_table) {
      result.push(edge);
      continue;
    }

    const resolvedTargets = [...new Set(edge.resolved_target_tables ?? [])]
      .filter((table) => typeof table === "string" && table.length > 0);
    if (resolvedTargets.length === 0) {
      throw new Error(
        `Exact edge has no proven target table: ${edge.source_table}:${edge.source_id}${edge.source_pointer}`,
      );
    }
    for (const targetTable of resolvedTargets) {
      result.push({
        ...edge,
        target_table: targetTable,
        expanded_from_resolved_target_tables: resolvedTargets,
      });
    }
  }
  return result;
}

function normalizeRows(parsed) {
  if (Array.isArray(parsed)) {
    return parsed.map((value, index) => ({
      storageKey: String(index),
      rowId: inferRowId(value, String(index)),
      value,
    }));
  }
  if (!parsed || typeof parsed !== "object") {
    return [{ storageKey: "0", rowId: "0", value: parsed }];
  }
  return Object.entries(parsed).map(([storageKey, value]) => ({
    storageKey: String(storageKey),
    rowId: inferRowId(value, storageKey),
    value,
  }));
}

function inferRowId(value, fallback) {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    for (const field of ["Id", "ID", "id", "EntryId", "UId", "Uid", "UID"]) {
      const candidate = value[field];
      if (Number.isSafeInteger(candidate) || typeof candidate === "string") return String(candidate);
    }
  }
  return String(fallback);
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function parseJsonColumn(row, column) {
  return row ? JSON.parse(row[column]) : null;
}

function safeTable(value) {
  if (!/^[a-z_]+$/.test(value)) throw new Error(`Unsafe table name ${value}`);
  return value;
}

function requireBuild(value, build, label) {
  if (String(value.game_build) !== String(build)) {
    throw new Error(`${label} build ${value.game_build} does not match ${build}`);
  }
}

function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}

function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); }
}

function writeJson(file, value) {
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function hashFile(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function hashText(value) {
  return createHash("sha256").update(value).digest("hex");
}

function normalizePath(value) {
  return value.replaceAll("\\", "/");
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`);
    const key = arg.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function required(value, key) {
  if (!value[key]) throw new Error(`Missing --${key}`);
  return value[key];
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-semantic-evidence-index.mjs build --build <id> --decoded-root <dir> --reference-graph <json> --reference-occurrences <jsonl> --reference-candidates <jsonl> --expression-edges <json> --semantic-closure <json> --semantic-audit <json> --output <sqlite>
  node tools/bpsr-semantic-evidence-index.mjs verify --input <sqlite> [--verify-sources false]
  node tools/bpsr-semantic-evidence-index.mjs lookup --input <sqlite> --id <uid> [--table <name>]
  node tools/bpsr-semantic-evidence-index.mjs mechanic --input <sqlite> --source-id <source-id>
  node tools/bpsr-semantic-evidence-index.mjs self-test`);
  process.exit(exitCode);
}
