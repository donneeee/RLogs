#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const PHASES = [
  {
    id: "static-identity-and-namespace",
    rank: 1,
    dependencyKinds: new Set(["seed-has-no-decoded-primary-row", "seed-namespace-ambiguous"]),
    proof: "Resolve the exact current-build table namespace and primary row before interpreting values or ownership.",
  },
  {
    id: "static-produced-damage-route",
    rank: 2,
    dependencyKinds: new Set(["produced-damage-without-packet-row"]),
    proof: "Resolve every produced-damage child, owner, and recount parent without assigning damage by name similarity.",
  },
  {
    id: "static-formula-magnitude",
    rank: 3,
    dependencyKinds: new Set(["formula-magnitude-unresolved"]),
    proof: "Resolve exact units, selectors, stacking, caps, and formula inputs from current-build evidence.",
  },
  {
    id: "runtime-provider-recipient-scope",
    rank: 4,
    dependencyKinds: new Set(["formula-recipient-scope-unresolved"]),
    proof: "Prove provider, recipient or target, lifecycle, and stack ownership from canonical packet events.",
  },
  {
    id: "runtime-counterfactual-conservation",
    rank: 5,
    dependencyKinds: new Set(),
    proof: "Replay observed output and counterfactual output, then prove party-damage conservation before enabling rDPS credit.",
  },
];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveBuildContext(options));
else if (command === "verify") verify(
  path.resolve(required(options, "input")),
  options.index ? path.resolve(options.index) : null,
);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveBuildContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    index: path.resolve(required(parsed, "index")),
    semanticClosure: path.resolve(required(parsed, "semantic-closure")),
    routeLedger: path.resolve(required(parsed, "route-ledger")),
    formulaLedger: path.resolve(required(parsed, "formula-ledger")),
    staticFormulaEvidence: path.resolve(required(parsed, "static-formula-evidence")),
    staticWorklist: path.resolve(required(parsed, "static-worklist")),
    recipientLedger: path.resolve(required(parsed, "recipient-ledger")),
    output: path.resolve(required(parsed, "output")),
    batchSize: parsePositiveInteger(parsed["batch-size"] ?? "25", "batch-size"),
  };
}

function build(context) {
  const started = performance.now();
  for (const [label, file] of [
    ["semantic evidence index", context.index],
    ["semantic dependency closure", context.semanticClosure],
    ["produced damage proof routes", context.routeLedger],
    ["formula magnitude ledger", context.formulaLedger],
    ["static formula evidence", context.staticFormulaEvidence],
    ["static rDPS worklist", context.staticWorklist],
    ["recipient scope ledger", context.recipientLedger],
  ]) requireFile(file, label);

  const closure = readJson(context.semanticClosure, "semantic dependency closure");
  const routes = readJson(context.routeLedger, "produced damage proof routes");
  const formula = readJson(context.formulaLedger, "formula magnitude ledger");
  const staticFormula = readJson(context.staticFormulaEvidence, "static formula evidence");
  const staticWorklist = readJson(context.staticWorklist, "static rDPS worklist");
  const recipient = readJson(context.recipientLedger, "recipient scope ledger");
  requireBuild(closure, context.build, "semantic dependency closure", "game_build");
  requireBuild(routes, context.build, "produced damage proof routes", "game_build");
  requireBuild(formula, context.build, "formula magnitude ledger", "static_game_build");
  requireBuild(staticFormula, context.build, "static formula evidence", "game_build");
  requireBuild(staticWorklist, context.build, "static rDPS worklist", "game_build");
  requireBuild(recipient, context.build, "recipient scope ledger", "static_game_build");

  const db = new DatabaseSync(context.index, { readOnly: true });
  let report;
  try {
    const metadata = readMetadata(db);
    if (metadata.game_build !== context.build) {
      throw new Error(`Evidence index build ${metadata.game_build} does not match ${context.build}`);
    }
    report = generateReport({
      build: context.build,
      batchSize: context.batchSize,
      indexPath: context.index,
      indexMetadata: metadata,
      closure,
      routes,
      formula,
      staticFormula,
      staticWorklist,
      recipient,
      db,
      inputs: {
        semantic_evidence_index: fileDescriptor(context.index),
        semantic_dependency_closure: fileDescriptor(context.semanticClosure),
        produced_damage_proof_routes: fileDescriptor(context.routeLedger),
        formula_magnitude_ledger: fileDescriptor(context.formulaLedger),
        static_formula_evidence: fileDescriptor(context.staticFormulaEvidence),
        static_rdps_worklist: fileDescriptor(context.staticWorklist),
        recipient_scope_ledger: fileDescriptor(context.recipientLedger),
      },
    });
  } finally {
    db.close();
  }

  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`);
  verify(context.output, context.index);
  console.log(
    `Semantic resolution batches built for ${context.build}: ${report.summary.work_items} candidates, ` +
    `${report.summary.batches} batches, ${report.summary.semantic_dependency_groups} retained semantic dependency groups ` +
    `in ${Math.round(performance.now() - started)} ms.`,
  );
}

function generateReport({
  build,
  batchSize,
  indexPath,
  indexMetadata,
  closure,
  routes,
  formula,
  staticFormula,
  staticWorklist,
  recipient,
  db,
  inputs,
}) {
  const formulaByRule = uniqueBy(formula.candidates, "source_rule_id", "formula candidate");
  const staticFormulaByRule = uniqueBy(staticFormula.sources, "source_rule_id", "static formula source");
  const ownedOutputByRule = uniqueBy(
    staticWorklist.exact_produced_damage_candidates ?? [],
    "source_rule_id",
    "exact source-owned output route",
  );
  const recipientByRule = uniqueBy(recipient.candidates, "source_rule_id", "recipient candidate");
  const mechanicByRule = uniqueBy(closure.mechanics, "source_rule_id", "semantic mechanic");
  const routeByRule = uniqueBy(routes.routes, "source_rule_id", "produced damage route");
  const sourceRuleIds = [...new Set([
    ...formulaByRule.keys(),
    ...staticFormulaByRule.keys(),
    ...ownedOutputByRule.keys(),
    ...recipientByRule.keys(),
    ...mechanicByRule.keys(),
    ...routeByRule.keys(),
  ])].sort(compareText);

  const queries = createQueries(db);
  const workItems = sourceRuleIds.map((sourceRuleId) => buildWorkItem({
    sourceRuleId,
    formula: formulaByRule.get(sourceRuleId) ?? null,
    staticFormula: staticFormulaByRule.get(sourceRuleId) ?? null,
    ownedOutput: ownedOutputByRule.get(sourceRuleId) ?? null,
    recipient: recipientByRule.get(sourceRuleId) ?? null,
    mechanic: mechanicByRule.get(sourceRuleId) ?? null,
    route: routeByRule.get(sourceRuleId) ?? null,
    queries,
    indexPath,
  }));
  workItems.sort(compareWorkItems);

  const batches = [];
  for (const phase of PHASES) {
    const phaseItems = workItems.filter((item) => item.phase.id === phase.id);
    for (let offset = 0; offset < phaseItems.length; offset += batchSize) {
      const chunk = phaseItems.slice(offset, offset + batchSize);
      batches.push({
        batch_id: `${String(phase.rank).padStart(2, "0")}-${phase.id}-${String(Math.floor(offset / batchSize) + 1).padStart(3, "0")}`,
        phase: phase.id,
        phase_rank: phase.rank,
        proof_gate: phase.proof,
        item_count: chunk.length,
        source_rule_ids: chunk.map((item) => item.source_rule_id),
        packet_capture_allowed: phase.rank >= 4,
      });
    }
  }

  const semanticDependencyCounts = countValues(
    workItems.flatMap((item) => item.requirements.semantic_dependencies.map((dependency) => dependency.kind)),
  );
  const phaseCounts = countValues(workItems.map((item) => item.phase.id));
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-semantic-resolution-batches.mjs",
    game_build: build,
    policy: {
      no_guessing: true,
      no_unresolved_evidence_hidden: true,
      one_canonical_work_item_per_source_rule: true,
      earliest_unresolved_gate_first: true,
      static_proof_precedes_packet_capture: true,
      name_only_produced_routes_never_promote: true,
      evidence_index_is_acceleration_not_authority: true,
      matching_build_runtime_proof_required_for_promotion: true,
      counterfactual_conservation_required_for_rdps: true,
    },
    inputs,
    evidence_index: {
      path: normalizePath(indexPath),
      source_fingerprint: indexMetadata.source_fingerprint,
      counts: indexMetadata.counts,
    },
    summary: {
      work_items: workItems.length,
      formula_candidates: formula.candidates.length,
      formula_magnitudes_resolved: staticFormula.summary.formula_magnitudes_resolved,
      formula_static_gates_resolved: staticFormula.summary.static_gates_resolved,
      exact_source_owned_output_routes: ownedOutputByRule.size,
      recipient_scope_candidates: recipient.candidates.length,
      semantic_mechanics: closure.mechanics.length,
      produced_damage_route_sources: routes.routes.length,
      semantic_dependency_groups: sum(Object.values(semanticDependencyCounts)),
      batches: batches.length,
      batch_size: batchSize,
      phase_counts: phaseCounts,
      semantic_dependency_counts: semanticDependencyCounts,
      zero_hidden_omissions: true,
    },
    phases: PHASES.map((phase) => ({
      id: phase.id,
      rank: phase.rank,
      proof_gate: phase.proof,
      work_items: phaseCounts[phase.id] ?? 0,
    })),
    batches,
    work_items: workItems,
  };
  report.content_sha256 = contentHash(report);
  return report;
}

function buildWorkItem({ sourceRuleId, formula, staticFormula, ownedOutput, recipient, mechanic, route, queries, indexPath }) {
  const sourceId = recipient?.source_id ?? formula?.source_id ?? staticFormula?.source_id ?? ownedOutput?.source_id ?? mechanic?.source_id ?? route?.source_id ?? null;
  const sourceName = recipient?.source_name ?? formula?.source_name ?? staticFormula?.source_name ?? mechanic?.source_name ?? route?.source_name ?? null;
  const retainedSemanticDependencies = mechanic?.unresolved_dependencies ?? [];
  const semanticDependencies = retainedSemanticDependencies.filter((dependency) => {
    if (dependency.kind === "formula-magnitude-unresolved" && staticFormula?.static_gate_resolved) return false;
    if (
      dependency.kind === "produced-damage-without-packet-row" &&
      route?.proof_state === "current-build-exact-route" &&
      route?.promotion_eligible === true &&
      (route.exact_routes?.length ?? 0) > 0
    ) return false;
    return true;
  });
  const dependencyKinds = new Set(semanticDependencies.map((dependency) => dependency.kind));
  const identifiers = collectIdentifiers({ formula, ownedOutput, recipient, mechanic, route });
  const evidence = identifiers.map((id) => queryEvidence(id, queries));
  const evidenceSummary = {
    identifiers: identifiers.length,
    decoded_rows: sum(evidence.map((entry) => entry.decoded_rows)),
    outgoing_exact_edges: sum(evidence.map((entry) => entry.outgoing_exact_edges)),
    incoming_exact_edges: sum(evidence.map((entry) => entry.incoming_exact_edges)),
    ambiguous_occurrences: sum(evidence.map((entry) => entry.ambiguous_occurrences)),
    mechanic_memberships: sum(evidence.map((entry) => entry.mechanic_memberships)),
  };
  const phase = selectPhase({ dependencyKinds, formula, staticFormula, recipient });
  const priorityScore = scoreWorkItem({ phase, semanticDependencies, formula, recipient, evidenceSummary });
  return {
    source_rule_id: sourceRuleId,
    source_id: sourceId,
    source_name: sourceName,
    source_kind: mechanic?.source_kind ?? inferSourceKind(sourceId),
    source_type: mechanic?.source_type ?? null,
    phase: { id: phase.id, rank: phase.rank, proof_gate: phase.proof },
    priority_score: priorityScore,
    requirements: {
      semantic_dependencies: semanticDependencies,
      retained_semantic_dependencies: retainedSemanticDependencies,
      produced_damage_route: route ? {
        proof_state: route.proof_state,
        promotion_eligible: route.promotion_eligible,
        exact_route_count: route.exact_routes?.length ?? 0,
        exact_graph_path_count: route.exact_graph_paths?.length ?? 0,
        candidate_count: route.candidate_routes?.length ?? 0,
        candidates: (route.candidate_routes ?? []).map((candidate) => ({
          recount_id: candidate.recount_id,
          recount_name: candidate.recount_name,
          damage_ids: candidate.damage_ids ?? [],
          proof_state: candidate.proof_state,
          scope_warning: candidate.scope_warning ?? null,
        })),
        missing_edges: route.missing_edges ?? [],
        next_proof_action: route.next_proof_action ?? null,
      } : null,
      owned_output_route: ownedOutput ? {
        proof_state: "static-current-build-exact-owned-output-route",
        contribution_mode: ownedOutput.contribution_mode,
        contribution_tier: ownedOutput.contribution_tier,
        confidence: ownedOutput.confidence,
        ownership: "source-owned-output",
        transfer_credit_eligible: false,
        runtime_detection: ownedOutput.runtime_matcher?.runtime_detection ?? null,
        effect_ids: ownedOutput.runtime_matcher?.buff_ids ?? [],
        damage_ids: ownedOutput.runtime_matcher?.target_damage_ids ?? [],
        recount_ids: ownedOutput.runtime_matcher?.target_recount_ids ?? [],
        rdps_enablement: ownedOutput.rdps_enablement ?? null,
        next_proof_action: "Replay matching-build canonical damage rows and verify source/recount conservation. Credit the emitted output to its source owner only; do not award transferred support rDPS without independent provider-to-recipient proof.",
      } : null,
      formula: formula ? {
        outcome: formula.outcome,
        remaining_requirement: formula.remaining_requirement,
        static_blockers: formula.static_blockers ?? [],
        current_build_promotion_eligible: formula.current_build_promotion_eligible,
        static_formula_evidence: staticFormula ? {
          classification: staticFormula.classification,
          formula_magnitude_resolved: staticFormula.formula_magnitude_resolved,
          static_gate_resolved: staticFormula.static_gate_resolved,
          runtime_selector_required: staticFormula.runtime_selector_required,
          accepted_terms: staticFormula.accepted_terms ?? [],
          rejected_terms: staticFormula.rejected_terms ?? [],
          remaining_static_blockers: staticFormula.remaining_static_blockers ?? [],
          remaining_runtime_requirements: staticFormula.remaining_runtime_requirements ?? [],
          evidence_sha256: staticFormula.evidence_sha256,
        } : null,
      } : null,
      recipient_scope: recipient ? {
        scope_queue: recipient.scope_queue,
        transfer_gate: recipient.transfer_gate,
        scope_resolution: recipient.scope_resolution,
        remaining_requirement: recipient.remaining_requirement,
        current_build_promotion_eligible: recipient.current_build_promotion_eligible,
        declared_effect_ids: recipient.declared_effect_ids ?? [],
        runtime_related_effect_ids: recipient.runtime_related_effect_ids ?? [],
        effect_ids: recipient.effect_ids ?? [],
        component_routes: recipient.component_scope_routes ?? [],
      } : null,
      conservation_replay_required: true,
    },
    identifiers,
    evidence_summary: evidenceSummary,
    evidence_locator: {
      index: normalizePath(indexPath),
      lookup_ids: identifiers,
      command_template: "node tools/bpsr-semantic-evidence-index.mjs lookup --input <index> --id <id>",
      mechanic_command: mechanic
        ? `node tools/bpsr-semantic-evidence-index.mjs mechanic --input <index> --source-id ${mechanic.source_id}`
        : null,
    },
    indexed_evidence: evidence,
  };
}

function selectPhase({ dependencyKinds, formula, staticFormula, recipient }) {
  for (const phase of PHASES.slice(0, 4)) {
    if ([...phase.dependencyKinds].some((kind) => dependencyKinds.has(kind))) return phase;
  }
  if (formula && (formula.static_blockers?.length ?? 0) > 0 && !staticFormula?.static_gate_resolved) return PHASES[2];
  if (recipient && scopeRequiresRecipientProof(recipient.scope_queue)) return PHASES[3];
  return PHASES[4];
}

function scoreWorkItem({ phase, semanticDependencies, formula, recipient, evidenceSummary }) {
  let score = (6 - phase.rank) * 1000;
  score += semanticDependencies.length * 100;
  score += (formula?.historical_packet_observations?.length ?? 0) * 30;
  score += (formula?.retained_historical_proofs?.length ?? 0) * 50;
  score += recipient?.runtime_related_effect_ids?.length ?? 0;
  if (scopeRequiresRecipientProof(recipient?.scope_queue)) score += 200;
  if (recipient?.effective_transfer_eligibilities?.includes?.("external-recipient-candidate")) score += 150;
  if (recipient?.effective_transfer_eligibilities?.includes?.("external-target-state-candidate")) score += 150;
  score += Math.min(100, evidenceSummary.incoming_exact_edges + evidenceSummary.outgoing_exact_edges);
  score += Math.min(50, evidenceSummary.decoded_rows);
  return score;
}

function queryEvidence(id, queries) {
  return {
    id,
    decoded_rows: Number(queries.decoded.get(id).count),
    decoded_tables: queries.decodedTables.all(id).map((row) => row.table_name),
    outgoing_exact_edges: Number(queries.outgoing.get(id).count),
    incoming_exact_edges: Number(queries.incoming.get(id).count),
    ambiguous_occurrences: Number(queries.ambiguous.get(id, id).count),
    ambiguous_classifications: queries.ambiguousClasses.all(id, id).map((row) => ({
      classification: row.classification,
      count: Number(row.count),
    })),
    mechanic_memberships: Number(queries.mechanics.get(id).count),
  };
}

function createQueries(db) {
  return {
    decoded: db.prepare("SELECT COUNT(*) AS count FROM decoded_rows WHERE row_id=?"),
    decodedTables: db.prepare("SELECT DISTINCT table_name FROM decoded_rows WHERE row_id=? ORDER BY table_name"),
    outgoing: db.prepare("SELECT COUNT(*) AS count FROM exact_edges WHERE source_id=?"),
    incoming: db.prepare("SELECT COUNT(*) AS count FROM exact_edges WHERE target_id=?"),
    ambiguous: db.prepare("SELECT COUNT(*) AS count FROM ambiguous_occurrences WHERE candidate_id=? OR source_id=?"),
    ambiguousClasses: db.prepare("SELECT classification,COUNT(*) AS count FROM ambiguous_occurrences WHERE candidate_id=? OR source_id=? GROUP BY classification ORDER BY classification"),
    mechanics: db.prepare("SELECT COUNT(*) AS count FROM mechanic_rows WHERE row_id=?"),
  };
}

function collectIdentifiers({ formula, ownedOutput, recipient, mechanic, route }) {
  const values = [
    ...(formula?.effect_ids ?? []),
    ...(formula?.formula_term_ids ?? []),
    ...(formula?.declared_effect_references ?? []).flatMap(extractIdentifierValues),
    ownedOutput?.runtime_matcher?.source_entity_id,
    ...(ownedOutput?.runtime_matcher?.buff_ids ?? []),
    ...(ownedOutput?.runtime_matcher?.target_damage_ids ?? []),
    ...(ownedOutput?.runtime_matcher?.target_recount_ids ?? []),
    ...(recipient?.effect_ids ?? []),
    ...(recipient?.declared_effect_ids ?? []),
    ...(recipient?.runtime_related_effect_ids ?? []),
    ...(mechanic?.seeds ?? []).flatMap((seed) => seed.id),
    ...(mechanic?.decoded_rows ?? []).flatMap((row) => row.row_id),
    ...(route?.exact_routes ?? []).flatMap((candidate) => [candidate.recount_id, ...(candidate.damage_ids ?? [])]),
    ...(route?.candidate_routes ?? []).flatMap((candidate) => [candidate.recount_id, ...(candidate.damage_ids ?? [])]),
  ];
  return [...new Set(values.flatMap(extractIdentifierValues).filter(isIdentifier))].sort(compareIdentifiers);
}

function extractIdentifierValues(value) {
  if (value === null || value === undefined) return [];
  if (typeof value === "string" || typeof value === "number" || typeof value === "bigint") return [String(value)];
  if (Array.isArray(value)) return value.flatMap(extractIdentifierValues);
  if (typeof value === "object") {
    return Object.entries(value)
      .filter(([key]) => /(^|_)(id|ids|uid|uids|value|values)$/i.test(key))
      .flatMap(([, item]) => extractIdentifierValues(item));
  }
  return [];
}

function isIdentifier(value) {
  return /^-?\d+$/.test(value) && value !== "0";
}

function scopeRequiresRecipientProof(queue) {
  return typeof queue === "string" && (
    queue.includes("external-")
    || queue === "unresolved-provider-recipient"
    || queue === "unresolved-target-filtered-provider-recipient"
    || queue === "owner-local-formula-context-requires-recipient-proof"
    || queue === "mixed-source-output-and-open-owner-context"
    || queue === "mixed-or-unclassified-scope"
    || queue === "component-scoped-mixed"
  );
}

function inferSourceKind(sourceId) {
  return typeof sourceId === "string" && sourceId.includes(":") ? sourceId.split(":", 1)[0] : null;
}

function verify(input, indexPath) {
  const report = readJson(input, "semantic resolution batches");
  if (report.schema_version !== 1) throw new Error("Resolution batch schema_version must be 1");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Resolution batch content hash mismatch");
  if (report.summary.work_items !== report.work_items.length) throw new Error("Work-item count mismatch");
  if (report.summary.batches !== report.batches.length) throw new Error("Batch count mismatch");
  if (!report.summary.zero_hidden_omissions) throw new Error("zero_hidden_omissions must remain true");

  const sourceRules = new Set();
  let dependencyCount = 0;
  for (const item of report.work_items) {
    if (sourceRules.has(item.source_rule_id)) throw new Error(`Duplicate work item ${item.source_rule_id}`);
    sourceRules.add(item.source_rule_id);
    if (!item.phase?.id || !item.requirements?.conservation_replay_required) {
      throw new Error(`Incomplete proof gates for ${item.source_rule_id}`);
    }
    if (
      item.requirements.semantic_dependencies.some((dependency) => dependency.kind === "produced-damage-without-packet-row") &&
      !item.requirements.produced_damage_route
    ) {
      throw new Error(`Missing produced-damage proof route for ${item.source_rule_id}`);
    }
    dependencyCount += item.requirements.semantic_dependencies.length;
  }
  if (dependencyCount !== report.summary.semantic_dependency_groups) {
    throw new Error(`Semantic dependency conservation mismatch: ${dependencyCount} != ${report.summary.semantic_dependency_groups}`);
  }

  const batched = report.batches.flatMap((batch) => batch.source_rule_ids);
  if (batched.length !== report.work_items.length || new Set(batched).size !== report.work_items.length) {
    throw new Error("Every work item must occur in exactly one batch");
  }
  for (const sourceRuleId of batched) {
    if (!sourceRules.has(sourceRuleId)) throw new Error(`Batch references unknown work item ${sourceRuleId}`);
  }

  if (indexPath) {
    requireFile(indexPath, "semantic evidence index");
    const db = new DatabaseSync(indexPath, { readOnly: true });
    try {
      const metadata = readMetadata(db);
      if (metadata.game_build !== String(report.game_build)) throw new Error("Evidence index build mismatch");
      if (metadata.source_fingerprint !== report.evidence_index.source_fingerprint) {
        throw new Error("Evidence index source fingerprint mismatch");
      }
    } finally {
      db.close();
    }
  }
  console.log(
    `Semantic resolution batches verified for build ${report.game_build}: ${report.work_items.length} candidates, zero hidden omissions.`,
  );
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-resolution-batch-test-"));
  try {
    const indexPath = path.join(root, "index.sqlite");
    const db = new DatabaseSync(indexPath);
    db.exec(`
      CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;
      CREATE TABLE decoded_rows(table_name TEXT,storage_key TEXT,row_id TEXT,row_sha256 TEXT,row_json TEXT);
      CREATE TABLE exact_edges(source_table TEXT,source_id TEXT,source_field TEXT,source_pointer TEXT,relationship TEXT,target_table TEXT,target_id TEXT,proof TEXT,edge_json TEXT);
      CREATE TABLE ambiguous_occurrences(source_table TEXT,source_id TEXT,field TEXT,path_pattern TEXT,semantic_field_key TEXT,json_pointer TEXT,candidate_id TEXT,classification TEXT);
      CREATE TABLE mechanic_rows(source_id TEXT,table_name TEXT,row_id TEXT,depth INTEGER,reached_via TEXT,row_sha256 TEXT);
      INSERT INTO metadata VALUES('game_build','1'),('source_fingerprint','test'),('counts','{}');
      INSERT INTO decoded_rows VALUES('BuffTable','10','10','hash','{}');
      INSERT INTO ambiguous_occurrences VALUES('BuffTable','10','Value','/Value','BuffTable/Value','/Value','20','reference-like-unproven');
    `);
    db.close();

    const closure = {
      game_build: "1",
      mechanics: [{
        source_rule_id: "rule:1", source_id: "buff-source:10", source_name: "Test",
        source_kind: "test", source_type: "test", seeds: [{ id: "10" }],
        decoded_rows: [{ table: "BuffTable", row_id: "10" }],
        unresolved_dependencies: [{ kind: "formula-magnitude-unresolved", severity: "error" }],
      }, {
        source_rule_id: "rule:2", source_id: "buff-source:20", source_name: "Resolved route",
        source_kind: "test", source_type: "test", seeds: [{ id: "20" }],
        decoded_rows: [],
        unresolved_dependencies: [{ kind: "produced-damage-without-packet-row", severity: "error" }],
      }],
    };
    const formula = {
      static_game_build: "1",
      candidates: [{
        source_rule_id: "rule:1", source_id: "buff-source:10", source_name: "Test",
        effect_ids: [10], formula_term_ids: [], static_blockers: ["test"],
        outcome: "unresolved", remaining_requirement: "prove", current_build_promotion_eligible: false,
      }],
    };
    const recipient = {
      static_game_build: "1",
      candidates: [{
        source_rule_id: "rule:1", source_id: "buff-source:10", source_name: "Test",
        effect_ids: [10], declared_effect_ids: [10], runtime_related_effect_ids: [],
        scope_queue: "owner-local-formula-context-requires-recipient-proof", transfer_gate: {}, scope_resolution: null,
        remaining_requirement: "prove", current_build_promotion_eligible: false,
      }],
    };
    const routes = {
      game_build: "1",
      routes: [{
        source_rule_id: "rule:2", source_id: "buff-source:20", source_name: "Resolved route",
        proof_state: "current-build-exact-route", promotion_eligible: true,
        exact_routes: [{ damage_ids: ["2001"], recount_ids: ["200"] }],
        exact_graph_paths: [{ source_id: "20", target_id: "2001" }],
        candidate_routes: [], missing_edges: [], next_proof_action: "runtime replay",
      }],
    };
    const staticFormula = {
      game_build: "1",
      summary: { formula_magnitudes_resolved: 0, static_gates_resolved: 0 },
      sources: [{
        source_rule_id: "rule:1", source_id: "buff-source:10", source_name: "Test",
        classification: "unit-or-formula-model-required", formula_magnitude_resolved: false,
        static_gate_resolved: false, runtime_selector_required: false,
        accepted_terms: [], rejected_terms: [], remaining_static_blockers: ["test"],
        remaining_runtime_requirements: ["prove"], evidence_sha256: "test",
      }],
    };
    const staticWorklist = {
      game_build: "1",
      exact_produced_damage_candidates: [{
        source_rule_id: "rule:1",
        source_id: "buff-source:10",
        contribution_mode: "exact-produced-damage",
        contribution_tier: "exact",
        confidence: "exact",
        runtime_matcher: {
          source_entity_id: 10,
          runtime_detection: "active-buff",
          buff_ids: [10],
          target_damage_ids: [1001],
          target_recount_ids: [100],
        },
        rdps_enablement: "blocked-pending-recipient-scope-and-current-build-packet-replay",
      }],
    };
    const closurePath = path.join(root, "closure.json");
    const routePath = path.join(root, "routes.json");
    const formulaPath = path.join(root, "formula.json");
    const staticFormulaPath = path.join(root, "static-formula.json");
    const staticWorklistPath = path.join(root, "static-worklist.json");
    const recipientPath = path.join(root, "recipient.json");
    const output = path.join(root, "output.json");
    writeJson(closurePath, closure);
    writeJson(routePath, routes);
    writeJson(formulaPath, formula);
    writeJson(staticFormulaPath, staticFormula);
    writeJson(staticWorklistPath, staticWorklist);
    writeJson(recipientPath, recipient);
    build({
      build: "1", index: indexPath, semanticClosure: closurePath,
      routeLedger: routePath, formulaLedger: formulaPath, staticFormulaEvidence: staticFormulaPath,
      staticWorklist: staticWorklistPath, recipientLedger: recipientPath, output, batchSize: 25,
    });
    const verified = verify(output, indexPath);
    if (verified.work_items[0].phase.id !== "static-formula-magnitude") {
      throw new Error("Self-test did not select earliest unresolved gate");
    }
    if (verified.work_items[0].evidence_summary.decoded_rows !== 1) {
      throw new Error("Self-test evidence lookup failed");
    }
    if (
      verified.work_items[0].requirements.owned_output_route?.transfer_credit_eligible !== false
      || !verified.work_items[0].identifiers.includes("1001")
      || !verified.work_items[0].identifiers.includes("100")
    ) {
      throw new Error("Self-test lost the exact source-owned output route or mislabeled it as transferable");
    }
    const resolvedRoute = verified.work_items.find((item) => item.source_rule_id === "rule:2");
    if (!resolvedRoute || resolvedRoute.phase.id !== "runtime-counterfactual-conservation") {
      throw new Error("Exact current-build produced-damage route did not advance to runtime replay");
    }
    if (resolvedRoute.requirements.semantic_dependencies.some((dependency) =>
      dependency.kind === "produced-damage-without-packet-row")) {
      throw new Error("Exact current-build produced-damage route retained its resolved dependency");
    }
    if (!scopeRequiresRecipientProof("owner-local-formula-context-requires-recipient-proof")) {
      throw new Error("Owner-local formula context was incorrectly treated as closed recipient scope");
    }
    if (scopeRequiresRecipientProof("self-only-current-component-proof")) {
      throw new Error("Exact self-only component proof was incorrectly reopened");
    }
    console.log("bpsr-semantic-resolution-batches self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function readMetadata(db) {
  const rows = db.prepare("SELECT key,value FROM metadata ORDER BY key").all();
  const metadata = Object.fromEntries(rows.map((row) => [row.key, row.value]));
  return {
    ...metadata,
    counts: metadata.counts ? JSON.parse(metadata.counts) : {},
  };
}

function uniqueBy(values, key, label) {
  const result = new Map();
  for (const value of values ?? []) {
    const identifier = value[key];
    if (!identifier) throw new Error(`${label} is missing ${key}`);
    if (result.has(identifier)) throw new Error(`Duplicate ${label} ${identifier}`);
    result.set(identifier, value);
  }
  return result;
}

function countValues(values) {
  const result = {};
  for (const value of values) result[value] = (result[value] ?? 0) + 1;
  return Object.fromEntries(Object.entries(result).sort(([left], [right]) => compareText(left, right)));
}

function compareWorkItems(left, right) {
  return left.phase.rank - right.phase.rank ||
    right.priority_score - left.priority_score ||
    compareText(left.source_rule_id, right.source_rule_id);
}

function compareIdentifiers(left, right) {
  const leftNumber = Number(left);
  const rightNumber = Number(right);
  if (Number.isSafeInteger(leftNumber) && Number.isSafeInteger(rightNumber) && leftNumber !== rightNumber) {
    return leftNumber - rightNumber;
  }
  return compareText(left, right);
}

function compareText(left, right) {
  return String(left).localeCompare(String(right), "en");
}

function sum(values) {
  return values.reduce((total, value) => total + Number(value), 0);
}

function fileDescriptor(file) {
  return {
    path: normalizePath(file),
    bytes: statSync(file).size,
    sha256: hashFile(file),
  };
}

function contentHash(report) {
  const clone = structuredClone(report);
  delete clone.content_sha256;
  return hashText(stableStringify(clone));
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function requireBuild(value, buildId, label, field) {
  if (String(value[field]) !== String(buildId)) {
    throw new Error(`${label} build ${value[field]} does not match ${buildId}`);
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

function parsePositiveInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${label} must be a positive integer`);
  return parsed;
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
  node tools/bpsr-semantic-resolution-batches.mjs build --build <id> --index <sqlite> --semantic-closure <json> --route-ledger <json> --formula-ledger <json> --static-formula-evidence <json> --static-worklist <json> --recipient-ledger <json> --output <json> [--batch-size 25]
  node tools/bpsr-semantic-resolution-batches.mjs verify --input <json> [--index <sqlite>]
  node tools/bpsr-semantic-resolution-batches.mjs self-test`);
  process.exit(exitCode);
}
