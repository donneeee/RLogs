#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const RECEIPT_STATES = new Set(["proven", "rejected", "inconclusive"]);
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveBuildContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "init-receipts") initReceipts(options);
else if (command === "record-receipt") recordReceipt(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveBuildContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    router: path.resolve(required(parsed, "router")),
    batches: path.resolve(required(parsed, "batches")),
    receipts: parsed.receipts ? path.resolve(parsed.receipts) : null,
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  const started = performance.now();
  requireFile(context.router, "proof frontier router");
  requireFile(context.batches, "semantic resolution batches");
  const router = readJson(context.router, "proof frontier router");
  const batches = readJson(context.batches, "semantic resolution batches");
  requireBuild(router, context.build, "proof frontier router");
  requireBuild(batches, context.build, "semantic resolution batches");

  const receiptRegistry = context.receipts && existsSync(context.receipts)
    ? readReceiptRegistry(context.receipts, context.build)
    : { schema_version: 1, game_build: context.build, receipts: [] };
  const routeOverrides = collectRouteOverrides(router);
  const canonicalItems = uniqueBy(batches.work_items ?? [], "source_rule_id", "semantic work item");
  const receiptByItem = groupBy(receiptRegistry.receipts, (receipt) => receipt.work_item_key);

  const items = [...canonicalItems.values()].map((workItem) => {
    const effective = effectiveWorkflow(workItem, routeOverrides.get(workItem.source_rule_id));
    const proofInput = canonicalProofInput(context.build, workItem, effective);
    const proofInputSha256 = canonicalHash(proofInput);
    const receiptEvaluation = evaluateReceipts(
      receiptByItem.get(workItem.source_rule_id) ?? [],
      proofInputSha256,
    );
    return {
      work_item_key: workItem.source_rule_id,
      source_rule_id: workItem.source_rule_id,
      source_id: workItem.source_id,
      source_name: workItem.source_name,
      source_kind: workItem.source_kind,
      source_type: workItem.source_type,
      identifiers: workItem.identifiers ?? [],
      original_phase: workItem.phase,
      effective_workflow: effective,
      proof_input_sha256: proofInputSha256,
      receipt_state: receiptEvaluation.state,
      reusable_receipt: receiptEvaluation.reusable,
      stale_receipts: receiptEvaluation.stale,
      next_proof_action: effective.next_proof_action ?? nextProofAction(workItem),
      evidence_locator: workItem.evidence_locator,
    };
  }).sort(compareItems);

  const groups = makeExecutionGroups(items);
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-proof-attempt-ledger.mjs",
    game_build: context.build,
    policy: {
      per_item_content_addressed_receipts: true,
      unchanged_proven_rejected_or_inconclusive_attempts_are_reusable: true,
      changed_evidence_fingerprints_are_requeued: true,
      stale_and_inconclusive_evidence_is_never_hidden: true,
      receipt_reuse_never_promotes_a_runtime_rule: true,
      private_evidence_paths_are_redacted_from_generated_output: true,
      one_canonical_item_combines_formula_scope_and_conservation_questions: true,
      expensive_capture_begins_only_after_static_and_binary_exhaustion: true,
    },
    receipt_contract: {
      registry_schema_version: 1,
      statuses: [...RECEIPT_STATES].sort(compareText),
      required_receipt_fields: [
        "receipt_id",
        "work_item_key",
        "proof_input_sha256",
        "status",
        "recorded_at",
        "conclusion",
        "evidence",
      ],
      evidence_fields: ["kind", "path", "sha256"],
      note: "Evidence paths stay in the optional receipt registry. The generated ledger retains only hashes and byte counts.",
    },
    inputs: {
      proof_frontier_router: fileDescriptor(context.router),
      semantic_resolution_batches: fileDescriptor(context.batches),
      proof_receipts: context.receipts && existsSync(context.receipts)
        ? fileDescriptor(context.receipts)
        : { path: context.receipts ? normalizePath(context.receipts) : null, present: false },
    },
    summary: {
      canonical_work_items: items.length,
      execution_groups: groups.length,
      receipt_registry_entries: receiptRegistry.receipts.length,
      receipt_state_counts: countValues(items.map((item) => item.receipt_state)),
      workflow_counts: countValues(items.map((item) => item.effective_workflow.id)),
      pending_expensive_attempts: items.filter((item) => item.receipt_state === "pending").length,
      reusable_attempts: items.filter((item) => item.receipt_state.startsWith("reusable-")).length,
      stale_receipts: sum(items.map((item) => item.stale_receipts.length)),
      hidden_omissions: 0,
      duration_ms: Math.round(performance.now() - started),
    },
    execution_groups: groups,
    items,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(
    `Proof attempt ledger ready for build ${context.build}: ${items.length} canonical items, ` +
    `${groups.length} execution groups, ${report.summary.reusable_attempts} reusable attempts, ` +
    `${report.summary.stale_receipts} stale receipts, zero hidden omissions.`,
  );
}

function verify(file) {
  requireFile(file, "proof attempt ledger");
  const report = readJson(file, "proof attempt ledger");
  if (report.schema_version !== 1) throw new Error("Proof attempt ledger schema_version must be 1");
  if (!/^\d+$/.test(String(report.game_build))) throw new Error("Proof attempt ledger game_build is invalid");
  if (report.generated_by !== "tools/bpsr-proof-attempt-ledger.mjs") {
    throw new Error("Proof attempt ledger generated_by is invalid");
  }
  if (contentHash(report) !== report.content_sha256) throw new Error("Proof attempt ledger content hash mismatch");
  const items = uniqueBy(report.items ?? [], "work_item_key", "proof attempt item");
  if (items.size !== Number(report.summary?.canonical_work_items)) {
    throw new Error("Proof attempt ledger canonical item count mismatch");
  }
  for (const item of items.values()) {
    if (!/^[a-f0-9]{64}$/.test(item.proof_input_sha256 ?? "")) {
      throw new Error(`Invalid proof input fingerprint for ${item.work_item_key}`);
    }
    if (!item.effective_workflow?.id) throw new Error(`Missing workflow for ${item.work_item_key}`);
    if (!item.receipt_state) throw new Error(`Missing receipt state for ${item.work_item_key}`);
    const receipts = [item.reusable_receipt, ...(item.stale_receipts ?? [])].filter(Boolean);
    if (receipts.some((receipt) => receipt.evidence?.some((entry) => "path" in entry))) {
      throw new Error(`Private evidence path leaked for ${item.work_item_key}`);
    }
    if (receipts.some((receipt) => receipt.evidence?.some((entry) =>
      !["current", "content-changed", "missing"].includes(entry?.state) ||
      !/^[a-f0-9]{64}$/.test(entry?.sha256 ?? "") ||
      (entry.state === "missing"
        ? entry.current_sha256 !== null
        : !/^[a-f0-9]{64}$/.test(entry?.current_sha256 ?? ""))))) {
      throw new Error(`Invalid redacted receipt evidence state for ${item.work_item_key}`);
    }
  }
  const groupItems = new Set();
  for (const group of report.execution_groups ?? []) {
    for (const key of group.work_item_keys ?? []) {
      if (!items.has(key)) throw new Error(`Execution group references missing item ${key}`);
      if (groupItems.has(key)) throw new Error(`Proof attempt item ${key} is assigned to multiple groups`);
      groupItems.add(key);
    }
  }
  if (groupItems.size !== items.size) throw new Error("Not every proof attempt item belongs to one execution group");
  if (Number(report.summary?.hidden_omissions) !== 0) throw new Error("Hidden omissions must remain zero");
  return report;
}

function collectRouteOverrides(router) {
  const result = new Map();
  for (const [queueId, queue] of Object.entries(router.route_queues ?? {})) {
    for (const item of queue.items ?? []) {
      if (!item.source_rule_id) throw new Error(`Router queue ${queueId} has an item without source_rule_id`);
      if (result.has(item.source_rule_id)) throw new Error(`Duplicate router override ${item.source_rule_id}`);
      result.set(item.source_rule_id, {
        id: queueId.replaceAll("_", "-"),
        queue_id: queueId,
        purpose: queue.purpose,
        workflow: queue.workflow,
        next_proof_action: item.next_proof_action,
        routing_reason: item.routing_reason,
        candidate_damage_ids: item.candidate_damage_ids ?? [],
        exact_stalls: item.exact_stalls ?? [],
      });
    }
  }
  return result;
}

function effectiveWorkflow(workItem, routeOverride) {
  if (routeOverride) return routeOverride;
  const phaseId = workItem.phase?.id;
  const runtime = phaseId?.startsWith("runtime-") ?? false;
  return {
    id: phaseId,
    queue_id: phaseId,
    purpose: workItem.phase?.proof_gate,
    workflow: runtime
      ? "Capture one bounded source activation with canonical provider, recipient, target, lifecycle, output, and timestamps; use the same fixture for formula, scope, and conservation proof."
      : "Resolve the current-build static evidence for this canonical source rule before requesting packet capture.",
  };
}

function canonicalProofInput(buildId, workItem, effective) {
  return {
    schema_version: 1,
    game_build: String(buildId),
    source_rule_id: workItem.source_rule_id,
    source_id: workItem.source_id,
    source_kind: workItem.source_kind,
    source_type: workItem.source_type,
    identifiers: workItem.identifiers ?? [],
    phase: workItem.phase,
    effective_workflow: effective,
    requirements: workItem.requirements,
    indexed_evidence: workItem.indexed_evidence,
  };
}

function evaluateReceipts(receipts, fingerprint) {
  const reusable = [];
  const stale = [];
  for (const receipt of receipts) {
    const safe = validateReceipt(receipt);
    const evidence = safe.evidence.map(validateEvidence);
    const evidenceIsCurrent = evidence.every((entry) => entry.state === "current");
    const redacted = {
      receipt_id: safe.receipt_id,
      status: safe.status,
      recorded_at: safe.recorded_at,
      conclusion: safe.conclusion,
      evidence: evidence.map((entry) => ({
        kind: entry.kind,
        state: entry.state,
        bytes: entry.bytes,
        sha256: entry.sha256,
        current_sha256: entry.current_sha256,
      })),
    };
    if (safe.proof_input_sha256 === fingerprint && evidenceIsCurrent) reusable.push(redacted);
    else stale.push({
      ...redacted,
      prior_proof_input_sha256: safe.proof_input_sha256,
      stale_reasons: [
        ...(safe.proof_input_sha256 === fingerprint ? [] : ["proof-input-changed"]),
        ...(evidenceIsCurrent ? [] : ["evidence-content-changed-or-missing"]),
      ],
    });
  }
  reusable.sort((left, right) => compareText(right.recorded_at, left.recorded_at));
  stale.sort((left, right) => compareText(right.recorded_at, left.recorded_at));
  if (reusable.length === 0) return { state: "pending", reusable: null, stale };
  const selected = reusable[0];
  return { state: `reusable-${selected.status}`, reusable: selected, stale: [...reusable.slice(1), ...stale] };
}

function validateReceipt(receipt) {
  for (const field of ["receipt_id", "work_item_key", "proof_input_sha256", "status", "recorded_at", "conclusion"]) {
    if (!receipt?.[field]) throw new Error(`Proof receipt is missing ${field}`);
  }
  if (!/^[a-f0-9]{64}$/.test(receipt.proof_input_sha256)) {
    throw new Error(`Proof receipt ${receipt.receipt_id} has an invalid proof_input_sha256`);
  }
  if (!RECEIPT_STATES.has(receipt.status)) {
    throw new Error(`Proof receipt ${receipt.receipt_id} has invalid status ${receipt.status}`);
  }
  if (!Array.isArray(receipt.evidence) || receipt.evidence.length === 0) {
    throw new Error(`Proof receipt ${receipt.receipt_id} must cite at least one evidence artifact`);
  }
  return receipt;
}

function validateEvidence(evidence) {
  for (const field of ["kind", "path", "sha256"]) {
    if (!evidence?.[field]) throw new Error(`Proof receipt evidence is missing ${field}`);
  }
  if (!/^[a-f0-9]{64}$/.test(evidence.sha256)) {
    throw new Error(`Proof receipt evidence has an invalid sha256: ${evidence.kind}`);
  }
  const file = path.resolve(evidence.path);
  if (!existsSync(file) || !statSync(file).isFile()) {
    return {
      kind: evidence.kind,
      state: "missing",
      bytes: 0,
      sha256: evidence.sha256,
      current_sha256: null,
    };
  }
  const currentSha256 = hashFile(file);
  return {
    kind: evidence.kind,
    state: currentSha256 === evidence.sha256 ? "current" : "content-changed",
    bytes: statSync(file).size,
    sha256: evidence.sha256,
    current_sha256: currentSha256,
  };
}

function makeExecutionGroups(items) {
  const groups = groupBy(items, (item) => [
    item.effective_workflow.id,
    item.source_kind ?? "unknown",
    item.receipt_state,
  ].join("|"));
  return [...groups.entries()].map(([key, members]) => {
    const [workflowId, sourceKind, receiptState] = key.split("|");
    const sorted = [...members].sort(compareItems);
    return {
      group_id: `attempt:${slug(workflowId)}:${slug(sourceKind)}:${slug(receiptState)}`,
      workflow_id: workflowId,
      source_kind: sourceKind,
      receipt_state: receiptState,
      item_count: sorted.length,
      work_item_keys: sorted.map((item) => item.work_item_key),
      execution_policy: receiptState === "pending"
        ? "execute-or-collect-new-evidence"
        : "reuse-exact-receipt-unless-input-fingerprint-changes",
    };
  }).sort((left, right) => compareText(left.group_id, right.group_id));
}

function nextProofAction(workItem) {
  return workItem.requirements?.formula?.remaining_requirement
    ?? workItem.requirements?.recipient_scope?.remaining_requirement
    ?? workItem.phase?.proof_gate
    ?? "Review retained evidence without guessing or hiding unresolved rows.";
}

function readReceiptRegistry(file, buildId) {
  const value = readJson(file, "proof receipt registry");
  if (value.schema_version !== 1) throw new Error("Proof receipt registry schema_version must be 1");
  requireBuild(value, buildId, "proof receipt registry");
  uniqueBy(value.receipts ?? [], "receipt_id", "proof receipt");
  return value;
}

function initReceipts(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  const output = path.resolve(required(parsed, "output"));
  if (existsSync(output)) {
    const registry = readReceiptRegistry(output, buildId);
    console.log(`Proof receipt registry already exists for build ${buildId}: ${registry.receipts.length} receipts.`);
    return registry;
  }
  mkdirSync(path.dirname(output), { recursive: true });
  const registry = { schema_version: 1, game_build: buildId, receipts: [] };
  writeJson(output, registry);
  console.log(`Initialized proof receipt registry for build ${buildId}: ${normalizePath(output)}`);
  return registry;
}

function recordReceipt(parsed) {
  const ledgerFile = path.resolve(required(parsed, "ledger"));
  const receiptsFile = path.resolve(required(parsed, "receipts"));
  const evidenceManifestFile = path.resolve(required(parsed, "evidence-manifest"));
  const workItemKey = required(parsed, "work-item");
  const status = required(parsed, "status");
  const conclusion = required(parsed, "conclusion");
  if (!RECEIPT_STATES.has(status)) throw new Error(`Invalid receipt status ${status}`);

  const ledger = verify(ledgerFile);
  const item = (ledger.items ?? []).find((candidate) => candidate.work_item_key === workItemKey);
  if (!item) throw new Error(`Unknown proof work item ${workItemKey}`);
  const registry = existsSync(receiptsFile)
    ? readReceiptRegistry(receiptsFile, String(ledger.game_build))
    : { schema_version: 1, game_build: String(ledger.game_build), receipts: [] };
  const manifest = readJson(evidenceManifestFile, "proof evidence manifest");
  const evidenceItems = Array.isArray(manifest) ? manifest : manifest.evidence;
  if (!Array.isArray(evidenceItems) || evidenceItems.length === 0) {
    throw new Error("Proof evidence manifest must contain a non-empty evidence array");
  }
  const evidence = evidenceItems.map((entry) => {
    if (!entry?.kind || !entry?.path) throw new Error("Proof evidence manifest entries require kind and path");
    const file = path.resolve(path.dirname(evidenceManifestFile), entry.path);
    requireFile(file, `proof receipt evidence ${entry.kind}`);
    const sha256 = hashFile(file);
    if (entry.sha256 && entry.sha256 !== sha256) {
      throw new Error(`Declared proof evidence hash changed: ${file}`);
    }
    return { kind: entry.kind, path: file, sha256 };
  });
  const recordedAt = new Date().toISOString();
  const receipt = {
    receipt_id: `receipt:${hashText(`${workItemKey}|${item.proof_input_sha256}|${status}|${recordedAt}`).slice(0, 24)}`,
    work_item_key: workItemKey,
    proof_input_sha256: item.proof_input_sha256,
    status,
    recorded_at: recordedAt,
    conclusion,
    evidence,
  };
  validateReceipt(receipt);
  evidence.forEach(validateEvidence);
  registry.receipts.push(receipt);
  mkdirSync(path.dirname(receiptsFile), { recursive: true });
  writeJson(receiptsFile, registry);
  readReceiptRegistry(receiptsFile, String(ledger.game_build));
  console.log(`Recorded ${status} proof receipt ${receipt.receipt_id} for ${workItemKey}.`);
  return receipt;
}

function selfTest() {
  const root = path.join(process.cwd(), `.proof-attempt-self-test-${process.pid}`);
  mkdirSync(root, { recursive: true });
  try {
    const evidence = path.join(root, "evidence.json");
    const routerPath = path.join(root, "router.json");
    const batchesPath = path.join(root, "batches.json");
    const receiptsPath = path.join(root, "receipts.json");
    const output = path.join(root, "ledger.json");
    writeJson(evidence, { exact: true });
    writeJson(routerPath, {
      game_build: "1",
      route_queues: {
        runtime_packet_correlation: {
          purpose: "runtime proof",
          workflow: "capture",
          items: [{ source_rule_id: "mrs:test", next_proof_action: "capture once" }],
        },
      },
    });
    const workItem = {
      source_rule_id: "mrs:test",
      source_id: "talent:1",
      source_name: "Test",
      source_kind: "talent",
      source_type: "talent",
      identifiers: ["1"],
      phase: { id: "static-formula-magnitude", rank: 3, proof_gate: "prove" },
      requirements: { formula: { remaining_requirement: "prove formula" } },
      indexed_evidence: [],
      evidence_locator: {},
    };
    writeJson(batchesPath, { game_build: "1", work_items: [workItem] });
    const effective = collectRouteOverrides(readJson(routerPath, "router")).get("mrs:test");
    const fingerprint = canonicalHash(canonicalProofInput("1", workItem, effective));
    const initialLedger = path.join(root, "initial-ledger.json");
    build({ build: "1", router: routerPath, batches: batchesPath, receipts: null, output: initialLedger });
    const evidenceManifest = path.join(root, "evidence-manifest.json");
    writeJson(evidenceManifest, { evidence: [{ kind: "canonical-fixture", path: evidence }] });
    recordReceipt({
      ledger: initialLedger,
      receipts: receiptsPath,
      "work-item": "mrs:test",
      status: "inconclusive",
      conclusion: "No matching event in fixture",
      "evidence-manifest": evidenceManifest,
    });
    build({ build: "1", router: routerPath, batches: batchesPath, receipts: receiptsPath, output });
    const report = verify(output);
    if (report.items[0].receipt_state !== "reusable-inconclusive") throw new Error("Self-test receipt reuse failed");
    if ("path" in report.items[0].reusable_receipt.evidence[0]) throw new Error("Self-test leaked evidence path");
    writeJson(evidence, { exact: false, changed_after_receipt: true });
    const staleOutput = path.join(root, "stale-ledger.json");
    build({
      build: "1",
      router: routerPath,
      batches: batchesPath,
      receipts: receiptsPath,
      output: staleOutput,
    });
    const staleReport = verify(staleOutput);
    const staleItem = staleReport.items[0];
    if (staleItem.receipt_state !== "pending" || staleItem.stale_receipts.length !== 1 ||
      staleItem.stale_receipts[0].evidence[0].state !== "content-changed" ||
      staleItem.stale_receipts[0].evidence[0].current_sha256 ===
        staleItem.stale_receipts[0].evidence[0].sha256) {
      throw new Error("Self-test changed receipt evidence was not requeued as stale");
    }
    console.log("bpsr-proof-attempt-ledger self-test passed");
  } finally {
    try { process.getBuiltinModule("node:fs").rmSync(root, { recursive: true, force: true }); } catch {}
  }
}

function compareItems(left, right) {
  return Number(left.original_phase?.rank ?? 999) - Number(right.original_phase?.rank ?? 999)
    || compareText(left.work_item_key, right.work_item_key);
}

function uniqueBy(values, key, label) {
  const result = new Map();
  for (const value of values) {
    const identifier = value?.[key];
    if (!identifier) throw new Error(`${label} is missing ${key}`);
    if (result.has(identifier)) throw new Error(`Duplicate ${label} ${identifier}`);
    result.set(identifier, value);
  }
  return result;
}

function groupBy(values, key) {
  const result = new Map();
  for (const value of values) {
    const identifier = key(value);
    if (!result.has(identifier)) result.set(identifier, []);
    result.get(identifier).push(value);
  }
  return result;
}

function countValues(values) {
  const counts = {};
  for (const value of values) counts[value] = (counts[value] ?? 0) + 1;
  return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => compareText(left, right)));
}

function sum(values) {
  return values.reduce((total, value) => total + Number(value), 0);
}

function contentHash(report) {
  const copy = JSON.parse(JSON.stringify(report));
  delete copy.content_sha256;
  return canonicalHash(copy);
}

function canonicalHash(value) {
  return hashText(stableStringify(JSON.parse(JSON.stringify(value))));
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function fileDescriptor(file) {
  return { path: normalizePath(file), bytes: statSync(file).size, sha256: hashFile(file) };
}

function requireBuild(value, buildId, label) {
  if (String(value.game_build) !== String(buildId)) {
    throw new Error(`${label} build ${value.game_build} does not match ${buildId}`);
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
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
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

function slug(value) {
  return String(value).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "unknown";
}

function compareText(left, right) {
  return String(left).localeCompare(String(right), "en");
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
  node tools/bpsr-proof-attempt-ledger.mjs build --build <id> --router <json> --batches <json> [--receipts <json>] --output <json>
  node tools/bpsr-proof-attempt-ledger.mjs verify --input <json>
  node tools/bpsr-proof-attempt-ledger.mjs init-receipts --build <id> --output <json>
  node tools/bpsr-proof-attempt-ledger.mjs record-receipt --ledger <json> --receipts <json> --work-item <key> --status <proven|rejected|inconclusive> --conclusion <text> --evidence-manifest <json>
  node tools/bpsr-proof-attempt-ledger.mjs self-test`);
  process.exit(exitCode);
}
