#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
const sourceFileDescriptorCache = new Map();

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "inspect") inspect(path.resolve(required(options, "input")), options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    closure: path.resolve(required(parsed, "closure")),
    aggregate: path.resolve(required(parsed, "aggregate")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  requireFile(context.closure, "rDPS proof closure");
  const closure = readJson(context.closure, "rDPS proof closure");
  requireBuild(closure.game_build, context.build, "rDPS proof closure");
  const aggregatePresent = existsSync(context.aggregate);
  const aggregateDocument = aggregatePresent
    ? readJson(context.aggregate, "proof correlation aggregate")
    : { reports: [] };
  if (aggregatePresent) {
    const aggregateBuild = aggregateDocument.aggregate?.manifest_game_build ??
      aggregateDocument.manifest_game_build ?? aggregateDocument.game_build;
    requireBuild(aggregateBuild, context.build, "proof correlation aggregate");
  }

  const reportIndex = indexReportEvidence(aggregateDocument.reports ?? []);
  const obligationEntries = (closure.obligation_results ?? []).map((obligation) =>
    buildObligationEntry(obligation, reportIndex, context.output),
  ).sort((left, right) => compareText(left.obligation_id, right.obligation_id));
  const sharedModelRoutes = (closure.shared_model_results ?? []).map(buildSharedModelRoute)
    .sort((left, right) => compareText(left.model_key, right.model_key));
  const runtimeEffectRoutes = (closure.packet_observed_runtime_effect_results ?? [])
    .map((effect) => buildRuntimeEffectRoute(effect, obligationEntries))
    .sort((left, right) => compareNumericText(left.effect_id, right.effect_id));

  const document = {
    schema_version: 1,
    generated_by: "tools/bpsr-deferred-attribution-ledger.mjs",
    game_build: context.build,
    policy: {
      raw_rlog_is_authoritative_event_source: true,
      correlation_aggregate_is_an_index_not_an_attribution_authority: true,
      unresolved_effects_never_receive_guessed_credit: true,
      unresolved_effects_are_never_hidden_or_dropped: true,
      proof_receipt_promotes_by_deterministic_same_build_replay: true,
      historical_replay_requires_the_retained_source_rlog: true,
      missing_packet_evidence_requires_a_future_capture: true,
      every_closure_obligation_is_preserved_exactly_once: true,
      every_shared_proof_model_is_preserved_exactly_once: true,
      every_packet_observed_runtime_effect_is_preserved_exactly_once: true,
    },
    inputs: {
      proof_closure: fileDescriptor(context.closure),
      proof_correlation_aggregate: optionalFileDescriptor(context.aggregate),
    },
    summary: summarize(obligationEntries, sharedModelRoutes, runtimeEffectRoutes, aggregatePresent),
    attribution_states: {
      "attributed-after-proof": "All strict proof gates are closed; deterministic replay may emit attributed damage.",
      "proven-zero-transfer": "Current-build proof establishes that this route transfers no damage credit to another player.",
      "deferred-replayable": "Attribution remains disabled, but retained matching-build rlogs contain evidence that can be replayed after proof closes.",
      "deferred-needs-capture": "No retained matching-build rlog currently contains candidate evidence for this obligation.",
      "deferred-index-only": "Correlation evidence exists, but its authoritative source rlog is not currently available.",
    },
    shared_model_routes: sharedModelRoutes,
    runtime_effect_routes: runtimeEffectRoutes,
    obligation_entries: obligationEntries,
  };
  document.content_sha256 = contentHash(document);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(document, null, 2)}\n`, "utf8");
  verify(context.output);
  console.log(
    `Deferred attribution ledger built for ${context.build}: ${obligationEntries.length} obligations; ` +
    `${document.summary.deferred_replayable} replayable, ${document.summary.deferred_needs_capture} need capture, ` +
    `${document.summary.deferred_index_only} index-only, zero hidden omissions.`,
  );
}

function buildObligationEntry(obligation, reportIndex, outputPath) {
  const obligationId = String(obligation.obligation_id);
  const reportEvidence = reportIndex.get(obligationId) ?? [];
  const replaySources = reportEvidence.map((match) => replaySourceDescriptor(match, outputPath));
  const candidateEvidence = Boolean(obligation.gates?.candidate_evidence) || hasCandidateEvidence(obligation.evidence);
  const strictPromotion = Boolean(obligation.transfer_gate?.runtime_credit_allowed) ||
    obligation.status === "strictly-promotable" || obligation.status === "promoted";
  const zeroTransfer = obligation.transfer_class === "nontransfer" &&
    Boolean(obligation.gates?.transfer_eligibility) && !strictPromotion;
  const availableReplaySources = replaySources.filter((source) => source.source_available);
  let attributionState;
  if (strictPromotion) attributionState = "attributed-after-proof";
  else if (zeroTransfer) attributionState = "proven-zero-transfer";
  else if (candidateEvidence && availableReplaySources.length > 0) attributionState = "deferred-replayable";
  else if (candidateEvidence && replaySources.length > 0) attributionState = "deferred-index-only";
  else attributionState = "deferred-needs-capture";

  const missingProofGates = Object.entries(obligation.gates ?? {})
    .filter(([, value]) => value === false)
    .map(([key]) => key.replaceAll("_", "-"));
  return {
    obligation_id: obligationId,
    domain: obligation.domain ?? null,
    subject_kind: obligation.subject_kind ?? null,
    subject_id: obligation.subject_id ?? null,
    subject_name: obligation.subject_name ?? null,
    source_rule_ids: uniqueSorted(obligation.source_rule_ids ?? []),
    shared_model_keys: uniqueSorted(obligation.shared_model_keys ?? []),
    effect_ids: uniqueSorted(obligation.effect_ids ?? []),
    transfer_class: obligation.transfer_class ?? null,
    transfer_gate_kind: obligation.transfer_gate?.kind ?? null,
    closure_status: obligation.status ?? null,
    attribution_state: attributionState,
    runtime_credit_enabled: strictPromotion,
    retroactive_replay_eligible: attributionState === "deferred-replayable",
    candidate_packet_evidence_retained: candidateEvidence,
    missing_proof_gates: uniqueSorted(missingProofGates),
    blockers: uniqueSorted(obligation.blockers ?? []),
    selector_contract_sha256: obligation.correlation_match?.runtime_selector_contract_sha256 ?? null,
    evidence_summary: compactClosureEvidence(obligation.evidence ?? {}),
    matching_replay_sources: replaySources,
  };
}

function buildSharedModelRoute(model) {
  const receiptGates = uniqueSorted(
    (model.proof_receipt ?? []).flatMap((receipt) => receipt.still_required_runtime_gates ?? []),
  );
  const stillRequired = uniqueSorted([...(model.still_required_runtime_gates ?? []), ...receiptGates]);
  return {
    model_key: String(model.model_key),
    model_family: model.model_family ?? null,
    component_key: model.component_key ?? null,
    status: model.status ?? null,
    registry_only_proof_route: Boolean(model.registry_only_proof_route),
    runtime_manifest_obligations: Number(model.runtime_manifest_obligations ?? 0),
    runtime_obligation_ids: uniqueSorted(model.runtime_obligation_ids ?? []),
    still_required_runtime_gates: stillRequired,
    blockers: uniqueSorted(model.blockers ?? []),
    proof_receipt_ids: uniqueSorted((model.proof_receipt ?? []).map((receipt) => receipt.proof_id)),
    proof_states: uniqueSorted(model.proof_states ?? []),
  };
}

function buildRuntimeEffectRoute(effect, obligations) {
  const effectId = String(effect.effect_id);
  const related = obligations.filter((entry) => entry.effect_ids.includes(effectId));
  const replaySources = deduplicateReplaySources(related.flatMap((entry) => entry.matching_replay_sources));
  return {
    effect_id: effectId,
    source_match: effect.source_match ?? null,
    closure_status: effect.status ?? null,
    runtime_credit_enabled: Boolean(effect.gates?.strict_counterfactual_conservation),
    missing_proof_gates: uniqueSorted(
      Object.entries(effect.gates ?? {}).filter(([, value]) => value === false)
        .map(([key]) => key.replaceAll("_", "-")),
    ),
    blockers: uniqueSorted(effect.blockers ?? []),
    evidence: effect.evidence ?? {},
    related_obligation_ids: related.map((entry) => entry.obligation_id),
    replay_source_count: replaySources.length,
    available_replay_source_count: replaySources.filter((source) => source.source_available).length,
  };
}

function indexReportEvidence(reports) {
  const index = new Map();
  for (const wrapper of reports) {
    const sourcePath = wrapper.source_path ?? wrapper.report?.source_path;
    const sessionId = wrapper.session_id ?? wrapper.report?.session_id;
    for (const evidence of wrapper.report?.obligations ?? []) {
      if (!hasReportEvidence(evidence)) continue;
      const key = String(evidence.obligation_id);
      const list = index.get(key) ?? [];
      list.push({ sourcePath, sessionId, evidence });
      index.set(key, list);
    }
  }
  return index;
}

function hasReportEvidence(evidence) {
  return Number(evidence.direct_matches ?? 0) > 0 || Number(evidence.contextual_matches ?? 0) > 0 ||
    (evidence.observed_event_kinds ?? []).length > 0 ||
    (evidence.provider_recipient_observations ?? []).length > 0 ||
    Number(evidence.recipient_window_damage_events ?? 0) > 0 ||
    Number(evidence.target_window_damage_events ?? 0) > 0 ||
    Number(evidence.packet_damage_rows?.length ?? 0) > 0;
}

function hasCandidateEvidence(evidence = {}) {
  return (evidence.observed_event_kinds ?? []).length > 0 ||
    Number(evidence.provider_recipient_observations ?? 0) > 0 ||
    Number(evidence.recipient_window_damage_events ?? 0) > 0 ||
    Number(evidence.target_window_damage_events ?? 0) > 0 ||
    Number(evidence.packet_damage_rows ?? 0) > 0;
}

function replaySourceDescriptor(match, outputPath) {
  const rawPath = String(match.sourcePath ?? "");
  const sourcePath = rawPath ? path.resolve(path.dirname(outputPath), rawPath) : "";
  const fallbackPath = rawPath ? path.resolve(process.cwd(), rawPath) : "";
  const resolvedPath = sourcePath && existsSync(sourcePath) ? sourcePath : fallbackPath;
  const available = Boolean(resolvedPath && existsSync(resolvedPath));
  const cachedFile = available ? cachedSourceFileDescriptor(resolvedPath) : null;
  const evidence = match.evidence;
  return {
    source_path: rawPath,
    session_id: match.sessionId ?? null,
    source_available: available,
    source_size_bytes: cachedFile?.size_bytes ?? null,
    source_sha256: cachedFile?.sha256 ?? null,
    selector_contract: evidence.selector_contract ?? null,
    first_sequence: evidence.first_sequence ?? null,
    last_sequence: evidence.last_sequence ?? null,
    direct_matches: Number(evidence.direct_matches ?? 0),
    contextual_matches: Number(evidence.contextual_matches ?? 0),
    matched_identifiers: uniqueSorted(evidence.matched_identifiers ?? []),
    observed_event_kinds: uniqueSorted(evidence.observed_event_kinds ?? []),
    status_states: evidence.status_states ?? {},
    provider_recipient_observations: Number(evidence.provider_recipient_observations?.length ?? 0),
    status_instance_count: Number(evidence.status_instance_ids?.length ?? 0),
    recipient_window_damage_events: Number(evidence.recipient_window_damage_events ?? 0),
    recipient_window_damage: String(evidence.recipient_window_damage ?? "0"),
    target_window_damage_events: Number(evidence.target_window_damage_events ?? 0),
    target_window_damage: String(evidence.target_window_damage ?? "0"),
    formula_input_snapshots: Number(evidence.formula_input_snapshots?.length ?? 0),
    packet_damage_rows: Number(evidence.packet_damage_rows?.length ?? 0),
    projection_statuses: uniqueSorted(evidence.projection_statuses ?? []),
    ambiguous_status_removals: Number(evidence.ambiguous_status_removals ?? 0),
    ambiguous_provider_window_damage_events: Number(evidence.ambiguous_provider_window_damage_events ?? 0),
  };
}

function cachedSourceFileDescriptor(filePath) {
  const existing = sourceFileDescriptorCache.get(filePath);
  if (existing) return existing;
  const descriptor = { size_bytes: statSync(filePath).size, sha256: sha256File(filePath) };
  sourceFileDescriptorCache.set(filePath, descriptor);
  return descriptor;
}

function compactClosureEvidence(evidence) {
  return {
    coverage_state: evidence.coverage_state ?? null,
    observed_event_kinds: uniqueSorted(evidence.observed_event_kinds ?? []),
    missing_event_kinds: uniqueSorted(evidence.missing_event_kinds ?? []),
    provider_recipient_observations: Number(evidence.provider_recipient_observations ?? 0),
    eligible_external_provider_recipient_observations: Number(evidence.eligible_external_provider_recipient_observations ?? 0),
    status_states: evidence.status_states ?? {},
    recipient_window_damage_events: Number(evidence.recipient_window_damage_events ?? 0),
    target_window_damage_events: Number(evidence.target_window_damage_events ?? 0),
    formula_input_snapshots: Number(evidence.formula_input_snapshots ?? 0),
    packet_damage_rows: Number(evidence.packet_damage_rows ?? 0),
    projection_statuses: uniqueSorted(evidence.projection_statuses ?? []),
  };
}

function summarize(entries, models, effects, aggregatePresent) {
  const stateCounts = countBy(entries, (entry) => entry.attribution_state);
  const sources = deduplicateReplaySources(entries.flatMap((entry) => entry.matching_replay_sources));
  return {
    proof_correlation_aggregate_present: aggregatePresent,
    total_obligations: entries.length,
    attributed_after_proof: stateCounts["attributed-after-proof"] ?? 0,
    proven_zero_transfer: stateCounts["proven-zero-transfer"] ?? 0,
    deferred_replayable: stateCounts["deferred-replayable"] ?? 0,
    deferred_needs_capture: stateCounts["deferred-needs-capture"] ?? 0,
    deferred_index_only: stateCounts["deferred-index-only"] ?? 0,
    retroactive_replay_eligible: entries.filter((entry) => entry.retroactive_replay_eligible).length,
    matching_replay_sources: sources.length,
    source_files_available: sources.filter((source) => source.source_available).length,
    source_files_missing: sources.filter((source) => !source.source_available).length,
    shared_model_routes: models.length,
    registry_only_shared_model_routes: models.filter((model) => model.registry_only_proof_route).length,
    packet_observed_runtime_effect_routes: effects.length,
    hidden_omissions: 0,
  };
}

function verify(inputPath) {
  requireFile(inputPath, "deferred attribution ledger");
  const document = readJson(inputPath, "deferred attribution ledger");
  if (document.schema_version !== 1) throw new Error("Unsupported deferred attribution ledger schema");
  if (document.generated_by !== "tools/bpsr-deferred-attribution-ledger.mjs") throw new Error("Unexpected ledger generator");
  if (document.content_sha256 !== contentHash(document)) throw new Error("Ledger content hash mismatch");
  const entries = document.obligation_entries ?? [];
  const closurePath = document.inputs?.proof_closure?.path;
  requireFile(closurePath, "ledger proof closure input");
  if (sha256File(closurePath) !== document.inputs.proof_closure.sha256) {
    throw new Error("Ledger proof closure input has changed; rebuild the ledger");
  }
  const closure = readJson(closurePath, "ledger proof closure input");
  requireBuild(closure.game_build, document.game_build, "ledger proof closure input");
  assertUnique(entries, "obligation_id", "obligation entry");
  assertUnique(document.shared_model_routes ?? [], "model_key", "shared model route");
  assertUnique(document.runtime_effect_routes ?? [], "effect_id", "runtime effect route");
  if (entries.length !== Number(document.summary?.total_obligations)) throw new Error("Obligation summary count mismatch");
  if (entries.length !== Number(closure.obligation_results?.length ?? 0)) throw new Error("Proof closure obligation count mismatch");
  if (Number(document.summary?.hidden_omissions) !== 0) throw new Error("Hidden omissions must remain zero");
  for (const entry of entries) {
    const closureEntry = (closure.obligation_results ?? []).find((item) => String(item.obligation_id) === entry.obligation_id);
    if (!closureEntry) throw new Error(`Ledger obligation is absent from proof closure: ${entry.obligation_id}`);
    const expectedMissingGates = uniqueSorted(
      Object.entries(closureEntry.gates ?? {}).filter(([, value]) => value === false)
        .map(([key]) => key.replaceAll("_", "-")),
    );
    if (stableStringify(entry.missing_proof_gates) !== stableStringify(expectedMissingGates)) {
      throw new Error(`Missing proof gates were not preserved for ${entry.obligation_id}`);
    }
    if (entry.retroactive_replay_eligible !== (entry.attribution_state === "deferred-replayable")) {
      throw new Error(`Replay eligibility mismatch for ${entry.obligation_id}`);
    }
    if (entry.attribution_state === "deferred-replayable" &&
        !entry.matching_replay_sources.some((source) => source.source_available)) {
      throw new Error(`Replayable obligation lacks an available source rlog: ${entry.obligation_id}`);
    }
    if (entry.attribution_state === "deferred-needs-capture" && entry.retroactive_replay_eligible) {
      throw new Error(`Needs-capture obligation claims replay eligibility: ${entry.obligation_id}`);
    }
  }
  if ((document.shared_model_routes ?? []).length !== Number(closure.shared_model_results?.length ?? 0)) {
    throw new Error("Proof closure shared model count mismatch");
  }
  for (const model of document.shared_model_routes ?? []) {
    if (model.registry_only_proof_route && Number(model.runtime_manifest_obligations) !== 0) {
      throw new Error(`Registry-only model fabricated runtime obligations: ${model.model_key}`);
    }
    const closureModel = (closure.shared_model_results ?? []).find((item) => String(item.model_key) === model.model_key);
    if (!closureModel) throw new Error(`Ledger shared model is absent from proof closure: ${model.model_key}`);
    const receiptGates = uniqueSorted([
      ...(closureModel.still_required_runtime_gates ?? []),
      ...(closureModel.proof_receipt ?? []).flatMap((receipt) => receipt.still_required_runtime_gates ?? []),
    ]);
    if (stableStringify(model.still_required_runtime_gates) !== stableStringify(receiptGates)) {
      throw new Error(`Shared model runtime gates were not preserved: ${model.model_key}`);
    }
  }
  if ((document.runtime_effect_routes ?? []).length !== Number(closure.packet_observed_runtime_effect_results?.length ?? 0)) {
    throw new Error("Proof closure runtime effect count mismatch");
  }
  console.log(
    `Deferred attribution ledger verified: ${entries.length} obligations, ` +
    `${document.summary.deferred_replayable} replayable, ${document.summary.deferred_needs_capture} need capture.`,
  );
}

function inspect(inputPath, parsed) {
  verify(inputPath);
  const document = readJson(inputPath, "deferred attribution ledger");
  const state = parsed.state;
  const effect = parsed.effect === undefined ? null : String(parsed.effect);
  const limit = parsed.limit === undefined ? 25 : Number(parsed.limit);
  const entries = (document.obligation_entries ?? []).filter((entry) =>
    (!state || entry.attribution_state === state) && (!effect || entry.effect_ids.includes(effect)),
  ).slice(0, limit);
  console.log(JSON.stringify({ summary: document.summary, matches: entries }, null, 2));
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "bpsr-deferred-ledger-"));
  try {
    const source = path.join(root, "sample.rlog");
    writeFileSync(source, "sample-canonical-events\n", "utf8");
    const closurePath = path.join(root, "closure.json");
    const aggregatePath = path.join(root, "aggregate.json");
    const outputPath = path.join(root, "ledger.json");
    const base = (id, candidate) => ({
      obligation_id: id,
      domain: "test",
      subject_kind: "effect",
      subject_id: id,
      subject_name: id,
      source_rule_ids: [id],
      shared_model_keys: [],
      effect_ids: [id.endsWith("1") ? "1" : "2"],
      transfer_gate: { kind: "external-recipient-counterfactual", runtime_credit_allowed: false },
      transfer_class: "externally-transferable",
      status: candidate ? "partial-candidate-event-coverage" : "no-candidate-evidence",
      blockers: candidate ? ["counterfactual-open"] : ["candidate-packet-evidence-missing"],
      gates: { matching_build: true, candidate_evidence: candidate, packet_conservation: false },
      evidence: { observed_event_kinds: candidate ? ["status"] : [], packet_damage_rows: 0 },
      correlation_match: { runtime_selector_contract_sha256: id },
    });
    writeJson(closurePath, {
      schema_version: 1,
      generated_by: "test",
      game_build: "123",
      shared_model_results: [{
        model_key: "runtime-input:test",
        model_family: "runtime-input",
        component_key: "test",
        status: "shared-model-proof-received-runtime-open",
        registry_only_proof_route: true,
        runtime_manifest_obligations: 0,
        runtime_obligation_ids: [],
        still_required_runtime_gates: ["conservation"],
        blockers: ["shared-proof-runtime-gate-open:conservation"],
        proof_receipt: [{ proof_id: "receipt", still_required_runtime_gates: ["conservation"] }],
        proof_states: ["exact-current-build-canonical-runtime-input-route-proven"],
      }],
      packet_observed_runtime_effect_results: [{
        effect_id: "1", status: "runtime-external-open", blockers: ["conservation"],
        gates: { exact_route: true, strict_counterfactual_conservation: false }, evidence: {},
      }],
      obligation_results: [base("obligation-1", true), base("obligation-2", false)],
    });
    writeJson(aggregatePath, {
      aggregate: { manifest_game_build: "123" },
      reports: [{ source_path: source, session_id: "session-1", report: { obligations: [{
        obligation_id: "obligation-1", selector_contract: "{}", direct_matches: 1, contextual_matches: 0,
        first_sequence: 1, last_sequence: 2, matched_identifiers: ["effect:1"], observed_event_kinds: ["status"],
        status_states: { applied: 1 }, provider_recipient_observations: [], status_instance_ids: ["1"],
      }] } }],
    });
    build({ build: "123", closure: closurePath, aggregate: aggregatePath, output: outputPath });
    const result = readJson(outputPath, "self-test ledger");
    if (result.summary.deferred_replayable !== 1 || result.summary.deferred_needs_capture !== 1) {
      throw new Error("Self-test state counts are wrong");
    }
    if (result.summary.registry_only_shared_model_routes !== 1 || result.runtime_effect_routes.length !== 1) {
      throw new Error("Self-test deferred routes were not retained");
    }
    console.log("Deferred attribution ledger self-test passed.");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function deduplicateReplaySources(sources) {
  const map = new Map();
  for (const source of sources) {
    const key = `${source.source_path}\u0000${source.session_id ?? ""}`;
    if (!map.has(key)) map.set(key, source);
  }
  return [...map.values()].sort((left, right) =>
    compareText(left.source_path, right.source_path) || compareText(left.session_id ?? "", right.session_id ?? ""),
  );
}

function countBy(items, keyOf) {
  const counts = {};
  for (const item of items) counts[keyOf(item)] = (counts[keyOf(item)] ?? 0) + 1;
  return counts;
}

function assertUnique(items, key, label) {
  const seen = new Set();
  for (const item of items) {
    const value = String(item[key]);
    if (seen.has(value)) throw new Error(`Duplicate ${label} ${value}`);
    seen.add(value);
  }
}

function fileDescriptor(filePath) {
  return { path: filePath, size_bytes: statSync(filePath).size, sha256: sha256File(filePath) };
}

function optionalFileDescriptor(filePath) {
  return existsSync(filePath) ? fileDescriptor(filePath) : { path: filePath, present: false };
}

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function contentHash(document) {
  const copy = structuredClone(document);
  delete copy.content_sha256;
  return createHash("sha256").update(stableStringify(copy)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function uniqueSorted(values) {
  return [...new Set(values.filter((value) => value !== null && value !== undefined).map(String))].sort(compareText);
}

function compareText(left, right) {
  return String(left).localeCompare(String(right), "en");
}

function compareNumericText(left, right) {
  const a = BigInt(left);
  const b = BigInt(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

function readJson(filePath, label) {
  try { return JSON.parse(readFileSync(filePath, "utf8")); }
  catch (error) { throw new Error(`Unable to read ${label} ${filePath}: ${error.message}`); }
}

function writeJson(filePath, value) {
  writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function requireFile(filePath, label) {
  if (!existsSync(filePath) || !statSync(filePath).isFile()) throw new Error(`${label} not found: ${filePath}`);
}

function requireBuild(actual, expected, label) {
  if (String(actual) !== String(expected)) throw new Error(`${label} build ${actual} does not match ${expected}`);
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const key = token.slice(2);
    const next = args[index + 1];
    if (next === undefined || next.startsWith("--")) parsed[key] = true;
    else { parsed[key] = next; index += 1; }
  }
  return parsed;
}

function required(parsed, key) {
  const value = parsed[key];
  if (value === undefined || value === true || value === "") throw new Error(`Missing --${key}`);
  return String(value);
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-deferred-attribution-ledger.mjs build --build <id> --closure <json> --aggregate <json> --output <json>
  node tools/bpsr-deferred-attribution-ledger.mjs verify --input <json>
  node tools/bpsr-deferred-attribution-ledger.mjs inspect --input <json> [--state <state>] [--effect <id>] [--limit <n>]
  node tools/bpsr-deferred-attribution-ledger.mjs self-test`);
  process.exit(exitCode);
}
