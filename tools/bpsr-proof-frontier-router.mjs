#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "inspect") inspect(path.resolve(required(options, "input")), required(options, "queue"));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    workbench: path.resolve(required(parsed, "workbench")),
    formulaLedger: path.resolve(required(parsed, "formula-ledger")),
    staticFormulaEvidence: path.resolve(required(parsed, "static-formula-evidence")),
    recipientLedger: path.resolve(required(parsed, "recipient-ledger")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  const started = performance.now();
  const workbench = readBuildArtifact(context.workbench, context.build, "game_build", "proof frontier workbench");
  const formulaLedger = readBuildArtifact(context.formulaLedger, context.build, "static_game_build", "formula gap ledger");
  const staticFormulaEvidence = readBuildArtifact(context.staticFormulaEvidence, context.build, "game_build", "static formula evidence");
  const recipientLedger = readBuildArtifact(context.recipientLedger, context.build, "static_game_build", "recipient scope ledger");
  const formulaCandidates = (formulaLedger.candidates ?? []).filter((candidate) => !candidate.current_build_promotion_eligible);
  const recipientCandidates = (recipientLedger.candidates ?? []).filter((candidate) => !candidate.current_build_promotion_eligible);
  const routeQueues = routeProofQueues(workbench.routes ?? []);
  const staticFormulaByRule = new Map((staticFormulaEvidence.sources ?? []).map((source) => [source.source_rule_id, source]));
  const formulaQueues = formulaProofQueues(formulaCandidates, staticFormulaByRule);
  const recipientQueues = recipientProofQueues(recipientCandidates);
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-proof-frontier-router.mjs",
    game_build: context.build,
    policy: {
      routing_is_derived_acceleration_only: true,
      never_promotes_relationships_or_formulas: true,
      every_open_route_has_exactly_one_primary_queue: true,
      formula_and_recipient_dimensions_remain_independent: true,
      failed_or_unproven_evidence_remains_actionable: true,
      runtime_capture_requested_only_after_static_and_binary_routes_are_exhausted: true,
      zero_hidden_omissions: true,
    },
    inputs: {
      proof_frontier_workbench: fileDescriptor(context.workbench),
      formula_gap_ledger: fileDescriptor(context.formulaLedger),
      static_formula_evidence: fileDescriptor(context.staticFormulaEvidence),
      recipient_scope_ledger: fileDescriptor(context.recipientLedger),
    },
    summary: {
      open_routes: (workbench.routes ?? []).length,
      routed_open_routes: countItems(routeQueues),
      route_queue_counts: countQueues(routeQueues),
      total_formula_candidates: (formulaLedger.candidates ?? []).length,
      closed_formula_candidates: (formulaLedger.candidates ?? []).length - formulaCandidates.length,
      formula_candidates: formulaCandidates.length,
      routed_formula_candidates: countItems(formulaQueues),
      formula_queue_counts: countQueues(formulaQueues),
      formula_magnitudes_resolved: formulaCandidates.filter((candidate) => staticFormulaByRule.get(candidate.source_rule_id)?.formula_magnitude_resolved).length,
      formula_static_gates_resolved: formulaCandidates.filter((candidate) => staticFormulaByRule.get(candidate.source_rule_id)?.static_gate_resolved).length,
      total_recipient_candidates: (recipientLedger.candidates ?? []).length,
      closed_recipient_candidates: (recipientLedger.candidates ?? []).length - recipientCandidates.length,
      recipient_candidates: recipientCandidates.length,
      routed_recipient_candidates: countItems(recipientQueues),
      recipient_queue_counts: countQueues(recipientQueues),
      static_or_binary_route_items_before_runtime_capture:
        (routeQueues.table_field_adjudication?.items.length ?? 0) + (routeQueues.binary_dataflow?.items.length ?? 0),
      runtime_candidate_route_items: routeQueues.runtime_candidate_correlation?.items.length ?? 0,
      runtime_packet_route_items:
        (routeQueues.runtime_candidate_correlation?.items.length ?? 0) +
        (routeQueues.runtime_packet_correlation?.items.length ?? 0),
      runtime_unbounded_route_items: routeQueues.runtime_packet_correlation?.items.length ?? 0,
      hidden_omissions: 0,
    },
    route_queues: routeQueues,
    formula_queues: formulaQueues,
    recipient_queues: recipientQueues,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(context.output);
  console.log(
    `Proof frontier router built for ${context.build}: ${report.summary.routed_open_routes} routes, ` +
    `${report.summary.routed_formula_candidates} formula candidates, and ` +
    `${report.summary.routed_recipient_candidates} recipient candidates assigned without omissions in ` +
    `${Math.round(performance.now() - started)} ms.`,
  );
}

function routeProofQueues(routes) {
  const queues = {
    table_field_adjudication: queue(
      "route:table-field-adjudication",
      0,
      "Prove or reject untyped decoded-table fields before any broader search.",
      "node tools/bpsr-semantic-field-adjudications.mjs build --build <build> --semantic-schema <schema> --decoded-root <decoded-root> --rules <rules> --output <output>",
    ),
    binary_dataflow: queue(
      "route:binary-dataflow",
      1,
      "Use the current GameAssembly and dump only where a concrete numeric binary seed exists.",
      "python tools/il2cpp-table-reference-callsite-proof.py --binary <GameAssembly.dll> --dump <dump.cs> --candidates <filtered-candidates.jsonl> --game-build <build> --output <proof.json>",
    ),
    runtime_candidate_correlation: queue(
      "route:runtime-candidate-correlation",
      2,
      "Correlate a bounded current-build candidate set when static evidence proves possible outputs but not the emitted member.",
      "Capture one controlled activation and test only the listed candidate damage IDs against canonical source, provider, recipient, target, and timestamps.",
    ),
    runtime_packet_correlation: queue(
      "route:runtime-packet-correlation",
      3,
      "Capture a controlled activation only after current-build static and binary evidence cannot identify the emitted output.",
      "Capture source activation, canonical damage/effect events, provider, recipient, target, and timestamps in one bounded fixture.",
    ),
  };
  for (const route of routes) {
    const bridges = route.ambiguous_bridges ?? [];
    const targets = route.candidate_damage_targets ?? [];
    let target;
    let reason;
    if (bridges.length > 0) {
      target = queues.table_field_adjudication;
      reason = `${bridges.length} actionable untyped field occurrence(s) remain on the exact frontier`;
    } else if (targets.length > 0) {
      target = queues.runtime_candidate_correlation;
      reason = `${targets.length} candidate DamageAttrTable output(s) form a bounded runtime correlation set, but no numeric binary seed exists`;
    } else {
      target = queues.runtime_packet_correlation;
      reason = "no actionable table bridge or candidate DamageAttrTable output remains";
    }
    target.items.push({
      work_item_key: `route:${route.source_rule_id}`,
      source_rule_id: route.source_rule_id,
      source_id: route.source_id,
      source_name: route.source_name,
      proof_state: route.proof_state,
      routing_reason: reason,
      actionable_semantic_fields: unique(bridges.map((item) => item.semantic_field_key)),
      exact_stalls: (route.exact_stalls ?? []).map((item) => `${item.table}:${item.id}`),
      candidate_damage_ids: targets.map((item) => String(item.damage_id)),
      next_proof_action: route.next_proof_action,
      inspect_command: route.direct_inspect_command,
    });
  }
  return finalizeQueues(queues);
}

function formulaProofQueues(candidates, staticFormulaByRule = new Map()) {
  const queues = {
    runtime_selector_and_counterfactual: queue(
      "formula:runtime-selector-and-counterfactual",
      0,
      "Reuse the exact decoded magnitude; prove only its active selector, lifecycle, observed output, and conserved counterfactual.",
      "Correlate the typed formula evidence with the canonical event stream without rescanning unchanged game tables.",
    ),
    current_build_observed_equation: queue(
      "formula:current-build-observed-equation",
      1,
      "Reprove observed mechanics with exact before/after inputs and conserved output.",
      "Correlate lifecycle window, formula inputs, observed output, counterfactual output, and conservation replay.",
    ),
    current_build_targeted_observation: queue(
      "formula:current-build-targeted-observation",
      2,
      "Obtain a current-build observation for a mechanic absent from the retained packet corpus.",
      "Record the exact effect lifecycle plus all required_runtime_evidence fields in a bounded fixture.",
    ),
  };
  for (const candidate of candidates) {
    if (candidate.current_build_promotion_eligible) continue;
    const observed = (candidate.historical_packet_observations ?? []).some((item) =>
      Number(item.status_events ?? 0) > 0 || Number(item.mechanic_state_changes ?? 0) > 0 || Number(item.selected_attributes_examined ?? 0) > 0);
    const staticFormula = staticFormulaByRule.get(candidate.source_rule_id) ?? null;
    const target = staticFormula?.static_gate_resolved
      ? queues.runtime_selector_and_counterfactual
      : observed || (candidate.retained_historical_proofs ?? []).length > 0
        ? queues.current_build_observed_equation
        : queues.current_build_targeted_observation;
    target.items.push({
      work_item_key: `formula:${candidate.source_rule_id}`,
      source_rule_id: candidate.source_rule_id,
      source_id: candidate.source_id,
      source_name: candidate.source_name,
      effect_ids: (candidate.effect_ids ?? []).map(String),
      formula_term_ids: candidate.formula_term_ids ?? [],
      outcome: candidate.outcome,
      static_blockers: candidate.static_blockers ?? [],
      static_formula: staticFormula ? {
        classification: staticFormula.classification,
        formula_magnitude_resolved: staticFormula.formula_magnitude_resolved,
        static_gate_resolved: staticFormula.static_gate_resolved,
        runtime_selector_required: staticFormula.runtime_selector_required,
        remaining_static_blockers: staticFormula.remaining_static_blockers,
        evidence_sha256: staticFormula.evidence_sha256,
      } : null,
      required_runtime_evidence: candidate.required_runtime_evidence ?? [],
      remaining_requirement: candidate.remaining_requirement,
      routing_reason: staticFormula?.static_gate_resolved
        ? "current-build static magnitude is decoded; only runtime selector, lifecycle, counterfactual, and conservation proof remain"
        : observed
          ? "historical packet evidence exists but does not prove a current-build equation"
          : "no packet observation currently proves the mechanic equation",
    });
  }
  return finalizeQueues(queues);
}

function recipientProofQueues(candidates) {
  const queues = {
    external_provider_recipient: queue(
      "recipient:external-provider-recipient",
      0,
      "Prioritize mechanics capable of transferring credit between players.",
      "Capture provider UID, recipient UID, lifecycle window, stacks, affected hits, and conservation replay.",
    ),
    external_target_state: queue(
      "recipient:external-target-state",
      1,
      "Prove target-debuff ownership and which attackers receive the benefit.",
      "Capture debuff provider, target entity, every benefiting attacker, lifecycle, stacks, and counterfactual output.",
    ),
    unresolved_scope: queue(
      "recipient:unresolved-scope",
      2,
      "Resolve provider/recipient scope before attribution.",
      "Capture canonical provider, recipient, target, lifecycle, and affected-output evidence without assigning credit.",
    ),
    known_nontransfer_or_component_routing: queue(
      "recipient:known-nontransfer-or-component-routing",
      3,
      "Retain proven self/source-owned/component routing for conservation and regression checks.",
      "Replay the typed transfer gate; do not award support credit unless external scope is newly proven.",
    ),
  };
  for (const candidate of candidates) {
    if (candidate.current_build_promotion_eligible) continue;
    const eligibility = new Set(candidate.effective_transfer_eligibilities ?? candidate.transfer_eligibilities ?? []);
    const scopeQueue = String(candidate.scope_queue ?? "");
    let target;
    if ([...eligibility].some((item) => item.includes("external-recipient")) || scopeQueue.includes("external-recipient")) {
      target = queues.external_provider_recipient;
    } else if ([...eligibility].some((item) => item.includes("external-target")) || scopeQueue.includes("external-target")) {
      target = queues.external_target_state;
    } else if (recipientScopeRequiresProof(scopeQueue)) {
      target = queues.unresolved_scope;
    } else {
      target = queues.known_nontransfer_or_component_routing;
    }
    target.items.push({
      work_item_key: `recipient:${candidate.source_rule_id}`,
      source_rule_id: candidate.source_rule_id,
      source_id: candidate.source_id,
      source_name: candidate.source_name,
      effect_ids: (candidate.effect_ids ?? []).map(String),
      scope_queue: candidate.scope_queue,
      effective_transfer_eligibilities: [...eligibility].sort(compareText),
      transfer_gate_kind: candidate.transfer_gate?.kind ?? null,
      remaining_requirement: candidate.remaining_requirement,
      routing_reason: `scope queue ${candidate.scope_queue ?? "unresolved"}`,
    });
  }
  return finalizeQueues(queues);
}

function recipientScopeRequiresProof(scopeQueue) {
  return scopeQueue.includes("unresolved")
    || scopeQueue === "owner-local-formula-context-requires-recipient-proof"
    || scopeQueue === "mixed-source-output-and-open-owner-context"
    || scopeQueue === "mixed-or-unclassified-scope"
    || scopeQueue === "component-scoped-mixed";
}

function queue(batchKey, priority, purpose, workflow) {
  return { batch_key: batchKey, priority, purpose, workflow, items: [] };
}

function finalizeQueues(queues) {
  return Object.fromEntries(Object.entries(queues).map(([key, value]) => [key, {
    ...value,
    item_count: value.items.length,
    items: value.items.sort((left, right) => compareText(left.source_rule_id, right.source_rule_id)),
  }]));
}

function verify(input) {
  const report = readJson(input, "proof frontier router");
  if (report.schema_version !== 1) throw new Error("Proof frontier router schema_version must be 1");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Proof frontier router content hash mismatch");
  if (!report.policy?.never_promotes_relationships_or_formulas || !report.policy?.zero_hidden_omissions) {
    throw new Error("Proof frontier router policy is unsafe");
  }
  verifyDimension(report.route_queues, report.summary.open_routes, "route");
  verifyDimension(report.formula_queues, report.summary.formula_candidates, "formula");
  verifyDimension(report.recipient_queues, report.summary.recipient_candidates, "recipient");
  if (report.summary.routed_open_routes !== report.summary.open_routes) throw new Error("Open route omission detected");
  if (report.summary.routed_formula_candidates !== report.summary.formula_candidates) throw new Error("Formula candidate omission detected");
  if (report.summary.routed_recipient_candidates !== report.summary.recipient_candidates) throw new Error("Recipient candidate omission detected");
  console.log(
    `Proof frontier router verified for ${report.game_build}: ${report.summary.open_routes} routes, ` +
    `${report.summary.formula_candidates} formula candidates, ${report.summary.recipient_candidates} recipient candidates, zero omissions.`,
  );
}

function verifyDimension(queues, expected, label) {
  const seen = new Set();
  let count = 0;
  for (const [name, queueValue] of Object.entries(queues ?? {})) {
    if (queueValue.item_count !== queueValue.items.length) throw new Error(`${label} queue ${name} count mismatch`);
    for (const item of queueValue.items) {
      if (seen.has(item.work_item_key)) throw new Error(`Duplicate ${label} work item ${item.work_item_key}`);
      seen.add(item.work_item_key);
      count += 1;
    }
  }
  if (count !== Number(expected)) throw new Error(`${label} routing mismatch: ${count}, expected ${expected}`);
}

function inspect(input, queueName) {
  const report = readJson(input, "proof frontier router");
  const matches = [report.route_queues, report.formula_queues, report.recipient_queues]
    .map((queues) => queues?.[queueName])
    .filter(Boolean);
  if (matches.length !== 1) throw new Error(`Unknown or ambiguous queue ${queueName}`);
  console.log(JSON.stringify(matches[0], null, 2));
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-frontier-router-"));
  try {
    const workbench = path.join(root, "workbench.json");
    const formula = path.join(root, "formula.json");
    const staticFormula = path.join(root, "static-formula.json");
    const recipient = path.join(root, "recipient.json");
    const output = path.join(root, "router.json");
    writeJson(workbench, { game_build: "123", routes: [
      routeFixture("one", [{ semantic_field_key: "Table/Field" }], []),
      routeFixture("two", [], [{ damage_id: "9" }]),
      routeFixture("three", [], []),
    ] });
    writeJson(formula, { static_game_build: "123", candidates: [
      formulaFixture("one", [{ status_events: 1 }]),
      formulaFixture("two", []),
    ] });
    writeJson(staticFormula, { game_build: "123", sources: [
      { source_rule_id: "mrs:one", formula_magnitude_resolved: true, static_gate_resolved: true, runtime_selector_required: true, remaining_static_blockers: [], evidence_sha256: "proof" },
      { source_rule_id: "mrs:two", formula_magnitude_resolved: false, static_gate_resolved: false, runtime_selector_required: false, remaining_static_blockers: ["test"], evidence_sha256: "open" },
    ] });
    writeJson(recipient, { static_game_build: "123", candidates: [
      recipientFixture("one", "external-recipient-requires-current-build-proof", ["external-recipient-candidate"]),
      recipientFixture("two", "unresolved-provider-recipient", []),
      recipientFixture("three", "self-only-formula-context-no-transfer", ["self-only-formula-context"]),
      recipientFixture("four", "owner-local-formula-context-requires-recipient-proof", ["owner-local-formula-context-recipient-scope-open"]),
      recipientFixture("five", "mixed-source-output-and-open-owner-context", ["direct-output-owned-by-source", "owner-local-formula-context-recipient-scope-open"]),
    ] });
    build({ build: "123", workbench, formulaLedger: formula, staticFormulaEvidence: staticFormula, recipientLedger: recipient, output });
    const report = readJson(output, "self-test output");
    if (report.route_queues.table_field_adjudication.item_count !== 1) throw new Error("Table route self-test failed");
    if (report.route_queues.binary_dataflow.item_count !== 0) throw new Error("Binary route self-test failed");
    if (report.route_queues.runtime_candidate_correlation.item_count !== 1) throw new Error("Runtime candidate route self-test failed");
    if (report.route_queues.runtime_packet_correlation.item_count !== 1) throw new Error("Runtime route self-test failed");
    if (report.formula_queues.runtime_selector_and_counterfactual.item_count !== 1) throw new Error("Formula static reuse self-test failed");
    if (report.recipient_queues.external_provider_recipient.item_count !== 1) throw new Error("Recipient external self-test failed");
    if (report.recipient_queues.unresolved_scope.item_count !== 3) throw new Error("Recipient open-scope self-test failed");
    if (report.recipient_queues.known_nontransfer_or_component_routing.item_count !== 1) throw new Error("Recipient proven nontransfer self-test failed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
  console.log("bpsr-proof-frontier-router self-test passed");
}

function routeFixture(id, bridges, targets) {
  return { source_rule_id: `mrs:${id}`, source_id: id, source_name: id, proof_state: "blocked", ambiguous_bridges: bridges,
    candidate_damage_targets: targets, exact_stalls: [], next_proof_action: "prove", direct_inspect_command: "inspect" };
}
function formulaFixture(id, observations) {
  return { source_rule_id: `mrs:${id}`, source_id: id, effect_ids: [], formula_term_ids: [], historical_packet_observations: observations,
    retained_historical_proofs: [], current_build_promotion_eligible: false, required_runtime_evidence: [], static_blockers: [], outcome: "open", remaining_requirement: "prove" };
}
function recipientFixture(id, scopeQueue, eligibility) {
  return { source_rule_id: `mrs:${id}`, source_id: id, effect_ids: [], scope_queue: scopeQueue,
    effective_transfer_eligibilities: eligibility, current_build_promotion_eligible: false, remaining_requirement: "prove" };
}

function countItems(queues) { return Object.values(queues).reduce((sum, item) => sum + item.items.length, 0); }
function countQueues(queues) { return Object.fromEntries(Object.entries(queues).map(([key, value]) => [key, value.items.length])); }
function unique(values) { return [...new Set(values)].sort(compareText); }
function compareText(left, right) { return String(left ?? "").localeCompare(String(right ?? "")); }
function contentHash(report) { const clone = structuredClone(report); delete clone.content_sha256; return createHash("sha256").update(JSON.stringify(clone)).digest("hex"); }
function fileDescriptor(file) { return { path: file, bytes: readFileSync(file).length, sha256: createHash("sha256").update(readFileSync(file)).digest("hex") }; }
function readBuildArtifact(file, buildId, key, label) { const value = readJson(file, label); if (String(value[key]) !== buildId) throw new Error(`${label} build mismatch`); return value; }
function readJson(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); return JSON.parse(readFileSync(file, "utf8")); }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return String(value[key]); }
function parseArgs(args) { const output = {}; for (let index = 0; index < args.length; index += 2) { const token = args[index]; if (!token?.startsWith("--")) throw new Error(`Unexpected argument ${token}`); const next = args[index + 1]; if (!next || next.startsWith("--")) throw new Error(`Missing value for ${token}`); output[token.slice(2)] = next; } return output; }
function usage(exitCode) { console.log(`Usage:\n  node tools/bpsr-proof-frontier-router.mjs build --build <id> --workbench <json> --formula-ledger <json> --static-formula-evidence <json> --recipient-ledger <json> --output <json>\n  node tools/bpsr-proof-frontier-router.mjs verify --input <json>\n  node tools/bpsr-proof-frontier-router.mjs inspect --input <json> --queue <queue-name>\n  node tools/bpsr-proof-frontier-router.mjs self-test`); process.exit(exitCode); }
