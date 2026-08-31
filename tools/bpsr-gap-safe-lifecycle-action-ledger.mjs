#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const GENERATED_BY = "tools/bpsr-gap-safe-lifecycle-action-ledger.mjs";
const SCHEMA_VERSION = 1;
const SOURCE_LEDGER_SCHEMA = 4;
const GAP_AUDIT_SCHEMA = 3;
const DEFAULT_MAX_SELECTED_ROWS = 500_000;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") await generate(options);
else if (command === "verify") await verifyCommand(options);
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function generate(values) {
  const sourceSummaryPath = path.resolve(required(values, "source-summary"));
  const sourceLedgerPath = path.resolve(required(values, "source-ledger"));
  const gapAuditPath = path.resolve(required(values, "gap-audit"));
  const outputLedgerPath = path.resolve(required(values, "output-ledger"));
  const outputSummaryPath = path.resolve(required(values, "output-summary"));
  const maxSelectedRows = positiveInteger(
    values["max-selected-rows"] ?? DEFAULT_MAX_SELECTED_ROWS,
    "--max-selected-rows",
  );
  refuseExisting([
    outputLedgerPath,
    outputSummaryPath,
    `${outputLedgerPath}.partial`,
    `${outputSummaryPath}.partial`,
  ]);
  fs.mkdirSync(path.dirname(outputLedgerPath), { recursive: true });
  fs.mkdirSync(path.dirname(outputSummaryPath), { recursive: true });

  const context = loadContext({
    sourceSummaryPath,
    sourceLedgerPath,
    gapAuditPath,
    maxSelectedRows,
  });
  const partial = `${outputLedgerPath}.partial`;
  const output = fs.openSync(partial, "wx");
  const outputHash = crypto.createHash("sha256");
  let outputBytes = 0;
  const emit = (row) => {
    const text = `${JSON.stringify(row)}\n`;
    fs.writeSync(output, text);
    outputHash.update(text);
    outputBytes += Buffer.byteLength(text);
  };
  let analysis;
  try {
    analysis = await scanSource(context, emit);
    fs.fsyncSync(output);
    fs.closeSync(output);
    fs.renameSync(partial, outputLedgerPath);
  } catch (error) {
    try { fs.closeSync(output); } catch {}
    throw error;
  }
  const report = buildReport({
    ...context,
    outputLedgerPath,
    outputBytes,
    outputSha256: outputHash.digest("hex"),
    analysis,
  });
  report.content_sha256 = contentHash(report);
  const summaryPartial = `${outputSummaryPath}.partial`;
  fs.writeFileSync(summaryPartial, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  fs.renameSync(summaryPartial, outputSummaryPath);
  await verifyArtifacts(outputSummaryPath, outputLedgerPath);
  process.stdout.write(`${JSON.stringify(report.summary, null, 2)}\n`);
}

function loadContext({ sourceSummaryPath, sourceLedgerPath, gapAuditPath, maxSelectedRows }) {
  requireFile(sourceSummaryPath, "source lifecycle/action summary");
  requireFile(sourceLedgerPath, "source lifecycle/action ledger");
  requireFile(gapAuditPath, "RLOG gap-window audit");
  const sourceSummary = readJson(sourceSummaryPath);
  const gapAudit = readJson(gapAuditPath);
  validateSourceSummary(sourceSummary, sourceLedgerPath);
  const gap = validateAndIndexGapAudit(gapAudit, String(sourceSummary.game_build));
  return {
    sourceSummaryPath,
    sourceLedgerPath,
    gapAuditPath,
    sourceSummary,
    gapAudit,
    gap,
    build: String(sourceSummary.game_build),
    effectId: Number(gapAudit.effect_id),
    maxSelectedRows,
  };
}

function validateSourceSummary(report, sourceLedgerPath) {
  if (
    Number(report?.schema_version) !== SOURCE_LEDGER_SCHEMA ||
    report?.generated_by !== "tools/bpsr-lifecycle-action-correlation-ledger.mjs" ||
    report?.content_sha256 !== contentHash(report) ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    report?.policy?.open_run_boundary_windows_are_identified_for_fail_closed_exclusion !== true ||
    report?.conclusion?.provider_ownership_proof_applied !== true ||
    report?.conclusion?.magnitude_or_formula_proven !== false ||
    report?.conclusion?.provider_rdps_credit_allowed !== false ||
    path.resolve(report?.output?.correlation_ledger?.path ?? "") !== sourceLedgerPath
  ) {
    throw new Error("Unsafe or incompatible source lifecycle/action ledger summary");
  }
}

function validateAndIndexGapAudit(audit, build) {
  const policy = audit?.policy ?? {};
  const summary = audit?.summary ?? {};
  if (
    Number(audit?.schema_version) !== GAP_AUDIT_SCHEMA ||
    audit?.generated_by !== "rlogs-bpsr-rlog-gap-window-audit" ||
    String(audit?.game_build) !== build ||
    audit?.damage_relationship !== "source" ||
    !Number.isSafeInteger(Number(audit?.effect_id)) ||
    Number(audit.effect_id) <= 0 ||
    policy.sealed_rlogs_are_streamed_one_event_at_a_time !== true ||
    policy.every_data_gap_and_recorder_pause_is_an_exclusion_boundary !== true ||
    policy.status_lifecycles_never_cross_exclusion_or_run_boundaries !== true ||
    policy.complete_gap_bounded_lifecycle_is_not_counterfactual_formula_proof !== true ||
    policy.packet_absence_is_not_zero !== true ||
    policy.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    policy.current_snapshots_are_never_backfilled_into_historical_windows !== true ||
    policy.formula_authority !== false ||
    policy.runtime_authority !== false ||
    policy.provider_rdps_credit_allowed !== false ||
    summary.exact_gap_bounded_lifecycle_windows_identified !== true ||
    summary.formula_authority !== false ||
    summary.runtime_authority !== false ||
    summary.provider_rdps_credit_allowed !== false
  ) {
    throw new Error("Unsafe or incompatible RLOG gap-window audit");
  }
  const windows = new Map();
  let expectedMemberships = 0;
  let windowCount = 0;
  for (const session of audit.sessions ?? []) {
    if (typeof session.session_id !== "string" || !session.session_id) {
      throw new Error("Gap-window audit session identity is missing");
    }
    for (const window of session.complete_gap_bounded_windows ?? []) {
      validateGapWindow(window);
      const key = gapWindowKey(session.session_id, Number(audit.effect_id), window);
      if (windows.has(key)) throw new Error(`Duplicate gap-safe window ${key}`);
      windows.set(key, {
        ...window,
        session_id: session.session_id,
        matched_rows: 0,
      });
      expectedMemberships += Number(window.damage_events_while_active);
      windowCount += 1;
    }
  }
  if (
    windowCount !== Number(summary.selected_effect_complete_gap_bounded_lifecycle_count) ||
    expectedMemberships !== Number(summary.selected_effect_damage_events_while_active)
  ) {
    throw new Error("Gap-window audit membership conservation failed");
  }
  return { windows, expectedMemberships, windowCount };
}

function validateGapWindow(window) {
  if (
    window?.gap_bounded !== true ||
    window?.effect_endpoint_damage_role !== "damage_actor" ||
    window?.controlled_counterfactual_pair_proven !== false ||
    window?.formula_authority !== false ||
    !safeInteger(window?.instance_id) ||
    !safeInteger(window?.source_actor_id) ||
    !safeInteger(window?.source_entity_uuid) ||
    !safeInteger(window?.target_actor_id) ||
    !safeInteger(window?.target_entity_uuid) ||
    !safeInteger(window?.applied_envelope_sequence) ||
    !safeInteger(window?.terminal_envelope_sequence) ||
    Number(window.terminal_envelope_sequence) <= Number(window.applied_envelope_sequence) ||
    !safeInteger(window?.damage_events_while_active) ||
    Number(window.damage_events_while_active) <= 0
  ) {
    throw new Error("Gap-window audit contains an unsafe or incomplete window");
  }
}

async function scanSource(context, emit) {
  const inputHash = crypto.createHash("sha256");
  const input = fs.createReadStream(context.sourceLedgerPath);
  input.on("data", (chunk) => inputHash.update(chunk));
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  const windowMatches = new Map();
  const providerRelationships = Object.create(null);
  const selectedSessions = new Set();
  const uniqueDamage = new Map();
  let lineNumber = 0;
  let sourceManifestCount = 0;
  let sourceRunHeaderCount = 0;
  let sourceCorrelationCount = 0;
  let selectedRows = 0;
  let selectedThirdPartyRows = 0;
  let selectedOwnershipUnresolvedRows = 0;
  let duplicateDamageMembershipRows = 0;
  let selectedReportedAmount = 0n;
  let selectedActualAmount = 0n;
  let selectedRowsWithActualAmount = 0;

  for await (const line of lines) {
    lineNumber += 1;
    if (!line.trim()) continue;
    const row = JSON.parse(line);
    if (row.row_type === "manifest") {
      sourceManifestCount += 1;
      emit({
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY,
        row_type: "manifest",
        game_build: context.build,
        effect_id: context.effectId,
        damage_relationship: "source",
        source_correlation_schema_version: SOURCE_LEDGER_SCHEMA,
        policy: policy(),
      });
      continue;
    }
    if (row.row_type === "run_header") {
      sourceRunHeaderCount += 1;
      emit({
        schema_version: SCHEMA_VERSION,
        row_type: "run_header",
        game_build: context.build,
        session_id: row.session_id,
        protocol_pack_digest: row.protocol_pack_digest,
        source_path: row.source_path,
      });
      continue;
    }
    if (row.row_type !== "lifecycle_damage_correlation") {
      throw new Error(`Unknown source ledger row type at line ${lineNumber}`);
    }
    sourceCorrelationCount += 1;
    if (Number(row.lifecycle?.effect_id) !== context.effectId ||
      !row.relationship_roles?.includes("source-side")) continue;
    const key = correlationWindowKey(row);
    const window = context.gap.windows.get(key);
    if (!window) continue;
    const damageSequence = Number(row.damage?.canonical_source_rlog_sequence);
    if (!Number.isSafeInteger(damageSequence) ||
      damageSequence <= Number(window.applied_envelope_sequence) ||
      damageSequence >= Number(window.terminal_envelope_sequence)) {
      throw new Error(`Selected correlation falls outside gap-safe window ${key}`);
    }
    selectedRows += 1;
    if (selectedRows > context.maxSelectedRows) {
      throw new Error(
        `Selected row limit ${context.maxSelectedRows} exceeded; refusing hidden truncation`,
      );
    }
    window.matched_rows += 1;
    windowMatches.set(key, window.matched_rows);
    selectedSessions.add(row.session_id);
    increment(providerRelationships, row.provider_relationship ?? "<null>");
    if (row.authority?.third_party_provider_ownership_proven === true) {
      selectedThirdPartyRows += 1;
    }
    if (row.authority?.provider_ownership_proven !== true) {
      selectedOwnershipUnresolvedRows += 1;
    }
    const damageKey = `${row.session_id}|${damageSequence}`;
    const amounts = {
      reported: integerOrNull(row.damage?.reported_amount),
      actual: integerOrNull(row.damage?.actual_amount),
    };
    const previousDamage = uniqueDamage.get(damageKey);
    if (previousDamage) {
      duplicateDamageMembershipRows += 1;
      if (previousDamage.reported !== amounts.reported || previousDamage.actual !== amounts.actual) {
        throw new Error(`Conflicting duplicate damage amounts for ${damageKey}`);
      }
    } else {
      uniqueDamage.set(damageKey, amounts);
    }
    if (amounts.reported != null) selectedReportedAmount += BigInt(amounts.reported);
    if (amounts.actual != null) {
      selectedActualAmount += BigInt(amounts.actual);
      selectedRowsWithActualAmount += 1;
    }
    emit({
      ...row,
      schema_version: SCHEMA_VERSION,
      generated_by: GENERATED_BY,
      row_type: "gap_safe_lifecycle_damage_correlation",
      source_correlation_schema_version: SOURCE_LEDGER_SCHEMA,
      gap_window: {
        segment_index: window.segment_index,
        applied_envelope_sequence: window.applied_envelope_sequence,
        terminal_envelope_sequence: window.terminal_envelope_sequence,
        terminal_state: window.terminal_state,
        gap_bounded: true,
      },
      authority: {
        ...row.authority,
        complete_gap_bounded_lifecycle: true,
        controlled_counterfactual_pair_proven: false,
        magnitude_or_formula_proven: false,
        provider_rdps_credit_allowed: false,
      },
    });
  }
  if (sourceManifestCount !== 1 ||
    sourceRunHeaderCount !== Number(context.sourceSummary.summary?.run_count) ||
    sourceCorrelationCount !== Number(context.sourceSummary.summary?.correlation_row_count)) {
    throw new Error("Source lifecycle/action ledger row conservation failed");
  }
  for (const [key, window] of context.gap.windows) {
    if ((windowMatches.get(key) ?? 0) !== Number(window.damage_events_while_active)) {
      throw new Error(
        `Gap-safe membership mismatch for ${key}: expected ` +
        `${window.damage_events_while_active}, observed ${windowMatches.get(key) ?? 0}`,
      );
    }
  }
  if (selectedRows !== context.gap.expectedMemberships) {
    throw new Error("Selected gap-safe row count does not conserve the gap audit");
  }
  return {
    input_line_count: lineNumber,
    input_sha256: inputHash.digest("hex"),
    source_manifest_count: sourceManifestCount,
    source_run_header_count: sourceRunHeaderCount,
    source_correlation_count: sourceCorrelationCount,
    gap_safe_window_count: context.gap.windowCount,
    selected_correlation_rows: selectedRows,
    selected_third_party_provider_rows: selectedThirdPartyRows,
    selected_ownership_unresolved_rows: selectedOwnershipUnresolvedRows,
    selected_session_count: selectedSessions.size,
    unique_damage_event_count: uniqueDamage.size,
    duplicate_damage_membership_rows: duplicateDamageMembershipRows,
    selected_reported_amount_membership_sum: selectedReportedAmount.toString(),
    selected_actual_amount_membership_sum: selectedActualAmount.toString(),
    selected_rows_with_actual_amount: selectedRowsWithActualAmount,
    provider_relationship_counts: sortedCounts(providerRelationships),
  };
}

function buildReport(context) {
  const { analysis } = context;
  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: context.build,
    effect_id: context.effectId,
    damage_relationship: "source",
    inputs: {
      source_lifecycle_action_summary: descriptor(context.sourceSummaryPath),
      source_lifecycle_action_ledger: {
        path: context.sourceLedgerPath,
        bytes: fs.statSync(context.sourceLedgerPath).size,
        sha256: analysis.input_sha256,
        schema_version: SOURCE_LEDGER_SCHEMA,
      },
      rlog_gap_window_audit: descriptor(context.gapAuditPath),
    },
    output: {
      gap_safe_correlation_ledger: {
        path: context.outputLedgerPath,
        bytes: context.outputBytes,
        sha256: context.outputSha256,
        schema_version: SCHEMA_VERSION,
      },
    },
    policy: {
      ...policy(),
      maximum_selected_rows: context.maxSelectedRows,
      row_limit_exhaustion_behavior: "fail-without-output-promotion",
    },
    source_protocol_pack_digests:
      structuredClone(context.sourceSummary.summary?.protocol_pack_digests ?? []),
    summary: analysis,
    conclusion: {
      exact_gap_safe_source_side_membership_conserved: true,
      provider_ownership_proven_for_every_selected_row:
        analysis.selected_ownership_unresolved_rows === 0,
      third_party_provider_gap_safe_correlations_available:
        analysis.selected_third_party_provider_rows > 0,
      controlled_counterfactual_pair_proven: false,
      magnitude_or_formula_proven: false,
      operation_order_stacking_and_rounding_proven: false,
      packet_conservation_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
  };
}

function policy() {
  return {
    exact_numeric_ids_build_and_session_identity_are_authoritative: true,
    localized_names_are_runtime_keys: false,
    source_side_and_target_side_joins_are_independent: true,
    endpoint_allegiance_is_assumed: false,
    remote_player_cast_packets_required: false,
    remote_player_cast_packets_synthesized: false,
    packet_absence_is_zero: false,
    every_data_gap_recorder_pause_and_run_boundary_is_an_exclusion_boundary: true,
    only_complete_gap_bounded_status_windows_are_selected: true,
    gap_safe_temporal_membership_proves_causal_formula: false,
    current_character_snapshots_substituted_into_older_runs: false,
    provider_rdps_credit_allowed: false,
    runtime_authority: false,
  };
}

async function verifyCommand(values) {
  const summary = path.resolve(required(values, "summary"));
  const ledger = path.resolve(required(values, "ledger"));
  await verifyArtifacts(summary, ledger);
  const report = readJson(summary);
  process.stdout.write(
    `Gap-safe lifecycle/action ledger verified for build ${report.game_build}, ` +
    `effect ${report.effect_id}: ${report.summary.selected_correlation_rows} memberships, ` +
    `${report.summary.selected_third_party_provider_rows} proven third-party, zero formula credit.\n`,
  );
}

async function verifyArtifacts(summaryPath, ledgerPath) {
  const report = readJson(summaryPath);
  if (
    Number(report?.schema_version) !== SCHEMA_VERSION ||
    report?.generated_by !== GENERATED_BY ||
    report?.content_sha256 !== contentHash(report) ||
    report?.policy?.only_complete_gap_bounded_status_windows_are_selected !== true ||
    report?.policy?.gap_safe_temporal_membership_proves_causal_formula !== false ||
    report?.conclusion?.exact_gap_safe_source_side_membership_conserved !== true ||
    report?.conclusion?.magnitude_or_formula_proven !== false ||
    report?.conclusion?.provider_rdps_credit_allowed !== false ||
    report?.conclusion?.runtime_promotion_allowed !== false ||
    report?.conclusion?.ui_rdps_display_allowed !== false
  ) {
    throw new Error("Unsafe or invalid gap-safe lifecycle/action summary");
  }
  for (const descriptorValue of Object.values(report.inputs ?? {})) {
    requireFile(path.resolve(descriptorValue.path), "gap-safe input");
    if (Number(descriptorValue.bytes) !== fs.statSync(path.resolve(descriptorValue.path)).size ||
      descriptorValue.sha256 !== await fileSha256(path.resolve(descriptorValue.path))) {
      throw new Error(`Gap-safe input descriptor mismatch: ${descriptorValue.path}`);
    }
  }
  const output = report.output?.gap_safe_correlation_ledger;
  if (
    path.resolve(output?.path ?? "") !== ledgerPath ||
    Number(output?.bytes) !== fs.statSync(ledgerPath).size ||
    output?.sha256 !== await fileSha256(ledgerPath)
  ) {
    throw new Error("Gap-safe output ledger descriptor mismatch");
  }
  const context = loadContext({
    sourceSummaryPath: path.resolve(report.inputs.source_lifecycle_action_summary.path),
    sourceLedgerPath: path.resolve(report.inputs.source_lifecycle_action_ledger.path),
    gapAuditPath: path.resolve(report.inputs.rlog_gap_window_audit.path),
    maxSelectedRows: Number(report.policy.maximum_selected_rows),
  });
  const expectedHash = crypto.createHash("sha256");
  let expectedBytes = 0;
  const analysis = await scanSource(context, (row) => {
    const text = `${JSON.stringify(row)}\n`;
    expectedHash.update(text);
    expectedBytes += Buffer.byteLength(text);
  });
  if (stableStringify(analysis) !== stableStringify(report.summary) ||
    expectedHash.digest("hex") !== output.sha256 || expectedBytes !== Number(output.bytes)) {
    throw new Error("Gap-safe selection does not reproduce from authoritative inputs");
  }
}

function gapWindowKey(sessionId, effectId, window) {
  return [
    sessionId,
    effectId,
    window.source_actor_id,
    window.source_entity_uuid,
    window.target_actor_id,
    window.target_entity_uuid,
    window.instance_id,
    window.applied_envelope_sequence,
  ].join("|");
}

function correlationWindowKey(row) {
  const lifecycle = row.lifecycle ?? {};
  return [
    row.session_id,
    lifecycle.effect_id,
    lifecycle.provider_actor_id,
    lifecycle.provider_entity_uuid,
    lifecycle.affected_entity_actor_id,
    lifecycle.affected_entity_uuid,
    lifecycle.status_instance_id,
    lifecycle.window_start_sequence,
  ].join("|");
}

function descriptor(file) {
  return {
    path: file,
    bytes: fs.statSync(file).size,
    sha256: sha256FileSync(file),
  };
}

function sha256FileSync(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function fileSha256(file) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash("sha256");
    const input = fs.createReadStream(file);
    input.on("data", (chunk) => hash.update(chunk));
    input.on("error", reject);
    input.on("end", () => resolve(hash.digest("hex")));
  });
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return crypto.createHash("sha256").update(stableStringify(clone)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function sortedCounts(counts) {
  return Object.entries(counts).sort(([a], [b]) => a.localeCompare(b, "en"))
    .map(([key, count]) => ({ key, count }));
}

function increment(counts, key) {
  counts[key] = (counts[key] ?? 0) + 1;
}

function integerOrNull(value) {
  if (value == null) return null;
  const number = Number(value);
  return Number.isSafeInteger(number) ? number : null;
}

function safeInteger(value) {
  return Number.isSafeInteger(Number(value));
}

function positiveInteger(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number <= 0) {
    throw new Error(`${label} must be a positive safe integer`);
  }
  return number;
}

function readJson(file) {
  requireFile(file, "JSON artifact");
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function requireFile(file, label) {
  if (!fs.existsSync(file) || !fs.statSync(file).isFile()) {
    throw new Error(`Missing ${label}: ${file}`);
  }
}

function refuseExisting(files) {
  for (const file of files) if (fs.existsSync(file)) throw new Error(`Refusing to overwrite ${file}`);
}

function required(values, key) {
  if (!values[key]) throw new Error(`Missing --${key}`);
  return String(values[key]);
}

function parseArgs(values) {
  const result = {};
  for (let index = 0; index < values.length; index += 2) {
    if (!values[index]?.startsWith("--") || values[index + 1] == null) {
      throw new Error("Options must be --name value pairs");
    }
    result[values[index].slice(2)] = values[index + 1];
  }
  return result;
}

async function selfTest() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "bpsr-gap-safe-ledger-"));
  try {
    const sourceLedger = path.join(root, "source.jsonl");
    const sourceSummary = path.join(root, "source-summary.json");
    const gapAuditPath = path.join(root, "gap.json");
    const outputLedger = path.join(root, "selected.jsonl");
    const outputSummary = path.join(root, "selected-summary.json");
    const baseCorrelation = {
      schema_version: SOURCE_LEDGER_SCHEMA,
      row_type: "lifecycle_damage_correlation",
      game_build: "24687926",
      session_id: "test-session",
      relationship_roles: ["source-side"],
      raw_provider_relationship: "raw-provider-distinct-from-damage-actor-and-target",
      provider_relationship: "provider-distinct-from-damage-actor-and-target",
      provider_ownership: { resolved_owner_character_id: "provider" },
      lifecycle: {
        effect_id: 2110125,
        provider_actor_id: "1",
        provider_entity_uuid: "101",
        affected_entity_actor_id: "2",
        affected_entity_uuid: "202",
        status_instance_id: 7,
        window_start_sequence: 10,
      },
      authority: {
        provider_ownership_proven: true,
        third_party_provider_ownership_proven: true,
        magnitude_or_formula_proven: false,
        provider_rdps_credit_allowed: false,
      },
    };
    const sourceRows = [
      { schema_version: SOURCE_LEDGER_SCHEMA, row_type: "manifest", policy: {} },
      {
        schema_version: SOURCE_LEDGER_SCHEMA,
        row_type: "run_header",
        game_build: "24687926",
        session_id: "test-session",
        protocol_pack_digest: "sha256:test",
        source_path: "test.rlog",
      },
      {
        ...baseCorrelation,
        damage: { canonical_source_rlog_sequence: 15, reported_amount: 100, actual_amount: 90 },
      },
      {
        ...baseCorrelation,
        lifecycle: { ...baseCorrelation.lifecycle, status_instance_id: 8 },
        damage: { canonical_source_rlog_sequence: 25, reported_amount: 50, actual_amount: 45 },
      },
    ];
    fs.writeFileSync(sourceLedger, `${sourceRows.map(JSON.stringify).join("\n")}\n`);
    const sourceReport = {
      schema_version: SOURCE_LEDGER_SCHEMA,
      generated_by: "tools/bpsr-lifecycle-action-correlation-ledger.mjs",
      game_build: "24687926",
      output: { correlation_ledger: { path: sourceLedger } },
      policy: {
        provider_rdps_credit_allowed: false,
        open_run_boundary_windows_are_identified_for_fail_closed_exclusion: true,
      },
      summary: { run_count: 1, correlation_row_count: 2, protocol_pack_digests: ["sha256:test"] },
      conclusion: {
        provider_ownership_proof_applied: true,
        magnitude_or_formula_proven: false,
        provider_rdps_credit_allowed: false,
      },
    };
    sourceReport.content_sha256 = contentHash(sourceReport);
    fs.writeFileSync(sourceSummary, `${JSON.stringify(sourceReport, null, 2)}\n`);
    const gapAudit = {
      schema_version: GAP_AUDIT_SCHEMA,
      generated_by: "rlogs-bpsr-rlog-gap-window-audit",
      game_build: "24687926",
      effect_id: 2110125,
      damage_relationship: "source",
      policy: {
        sealed_rlogs_are_streamed_one_event_at_a_time: true,
        every_data_gap_and_recorder_pause_is_an_exclusion_boundary: true,
        status_lifecycles_never_cross_exclusion_or_run_boundaries: true,
        complete_gap_bounded_lifecycle_is_not_counterfactual_formula_proof: true,
        packet_absence_is_not_zero: true,
        structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
        current_snapshots_are_never_backfilled_into_historical_windows: true,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
      },
      summary: {
        selected_effect_complete_gap_bounded_lifecycle_count: 1,
        selected_effect_damage_events_while_active: 1,
        exact_gap_bounded_lifecycle_windows_identified: true,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
      },
      sessions: [{
        session_id: "test-session",
        complete_gap_bounded_windows: [{
          segment_index: 1,
          instance_id: 7,
          source_actor_id: 1,
          source_entity_uuid: 101,
          target_actor_id: 2,
          target_entity_uuid: 202,
          applied_envelope_sequence: 10,
          terminal_envelope_sequence: 20,
          terminal_state: "removed",
          effect_endpoint_damage_role: "damage_actor",
          damage_events_while_active: 1,
          gap_bounded: true,
          controlled_counterfactual_pair_proven: false,
          formula_authority: false,
        }],
      }],
    };
    fs.writeFileSync(gapAuditPath, `${JSON.stringify(gapAudit, null, 2)}\n`);
    await generate({
      "source-summary": sourceSummary,
      "source-ledger": sourceLedger,
      "gap-audit": gapAuditPath,
      "output-ledger": outputLedger,
      "output-summary": outputSummary,
      "max-selected-rows": "10",
    });
    const report = readJson(outputSummary);
    assert.equal(report.summary.selected_correlation_rows, 1);
    assert.equal(report.summary.selected_third_party_provider_rows, 1);
    assert.equal(report.summary.selected_ownership_unresolved_rows, 0);
    process.stdout.write("bpsr gap-safe lifecycle/action ledger self-test passed\n");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

function usage(code) {
  process.stdout.write(
    "Usage:\n" +
      "  node tools/bpsr-gap-safe-lifecycle-action-ledger.mjs generate " +
      "--source-summary <json> --source-ledger <jsonl> --gap-audit <json> " +
      "--output-ledger <jsonl> --output-summary <json> [--max-selected-rows <n>]\n" +
      "  node tools/bpsr-gap-safe-lifecycle-action-ledger.mjs verify " +
      "--summary <json> --ledger <jsonl>\n" +
      "  node tools/bpsr-gap-safe-lifecycle-action-ledger.mjs self-test\n",
  );
  process.exit(code);
}
