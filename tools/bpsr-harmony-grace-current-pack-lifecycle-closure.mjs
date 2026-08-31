#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-harmony-grace-current-pack-lifecycle-closure.mjs";
const SCHEMA_VERSION = 1;
const EFFECT_ID = 3_003_052;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(values) {
  const build = required(values, "build");
  const auditPath = path.resolve(required(values, "audit"));
  const lifecycleDiffPath = path.resolve(required(values, "lifecycle-diff"));
  const protocolStatusPath = path.resolve(required(values, "protocol-status"));
  const segmentReportPath = path.resolve(required(values, "segment-report"));
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const analysis = analyze({
    build,
    audit: readJson(auditPath),
    auditPath,
    lifecycleDiff: readJson(lifecycleDiffPath),
    lifecycleDiffPath,
    protocolStatus: readJson(protocolStatusPath),
    segmentReport: readJson(segmentReportPath),
  });
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: build,
    effect_id: EFFECT_ID,
    identity: analysis.identity,
    inputs: {
      replay_audit: fileReceipt(auditPath),
      lifecycle_differential: fileReceipt(lifecycleDiffPath),
      protocol_status: fileReceipt(protocolStatusPath),
      run_segmentation: fileReceipt(segmentReportPath),
    },
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      target_allegiance_inferred: false,
      remote_player_cast_packet_required: false,
    },
    policy: {
      exact_numeric_ids_build_and_protocol_pack_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      unresolved_statuses_mapped_to_invented_effects: false,
      terminal_only_unresolved_rows_treated_as_permanently_active: false,
      current_character_snapshots_used: false,
      ordinary_damage_totals_may_change: false,
      packet_state_counterfactual_is_server_integer_observation: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
    },
    proof: analysis.proof,
    summary: analysis.summary,
    blockers: [
      "a controlled absent/present/absent damage sequence must select the final server integer counterfactual instead of treating the exact rational transfer as a server-observed integer",
      "repeat the exact current-pack proof for additional recipient classes, providers, and stacking combinations",
      "close the aggregate current-build conservation and protocol-event-coverage promotion suites",
      "enable only an explicit exact-build recipient allowlist after every required gate closes",
    ],
    decision: "exact-current-pack-lifecycle-regression-closed-formula-candidate-runtime-disabled",
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify(report.summary, null, 2)}\n`);
}

function analyze({
  build,
  audit,
  auditPath,
  lifecycleDiff,
  lifecycleDiffPath,
  protocolStatus,
  segmentReport,
}) {
  assert.match(build, /^\d+$/);
  assert.ok(Number(audit.schema_version) >= 27);
  assert.equal(audit.attribution_mode, "offline_candidate_gate_audit_not_production_attribution");
  assert.equal(audit.harmony_grace_candidate_audit_enabled, true);
  assert.ok(Array.isArray(audit.reports) && audit.reports.length === 1);
  const replay = audit.reports[0];
  assert.equal(String(replay.client_build), build);
  assert.equal(replay.runtime_target_match, false);
  assert.equal(replay.candidate_audit_target_match, true);
  assert.equal(replay.conserved, true);
  assert.ok(Array.isArray(replay.emitted_contribution_ledger));
  const ledger = replay.emitted_contribution_ledger;
  assert.equal(Number(replay.emitted_contribution_events_by_effect?.[EFFECT_ID]), ledger.length);
  assert.equal(Number(replay.harmony_grace_audit_gates?.emitted), ledger.length);
  assert.equal(Number(replay.summary.attributed_damage_event_count), ledger.length);
  assert.ok(ledger.length > 0);

  assert.equal(lifecycleDiff.schema_version, 2);
  assert.equal(lifecycleDiff.generated_by, "rlogs-bpsr-harmony-overlap-ledger-diff");
  assert.equal(lifecycleDiff.row_details_included, false);
  assert.deepEqual(lifecycleDiff.rows, []);
  assert.equal(String(lifecycleDiff.identity.client_build), build);
  assert.equal(lifecycleDiff.identity.session_id, replay.session_id);
  assert.equal(lifecycleDiff.identity.deployment_id, replay.deployment_id);
  assert.equal(lifecycleDiff.identity.protocol_pack_digest, replay.protocol_pack_digest);
  assert.equal(lifecycleDiff.identity.sealed, true);
  assert.equal(Number(lifecycleDiff.identity.rlog_event_count), Number(replay.event_count));
  assert.equal(Number(lifecycleDiff.comparison.effect_id), EFFECT_ID);
  assert.equal(Number(lifecycleDiff.comparison.candidate_rows), ledger.length);
  assert.equal(Number(lifecycleDiff.comparison.candidate_rows_matched_in_rlog), ledger.length);
  assert.equal(lifecycleDiff.comparison.trusted_is_exact_subset_of_candidate, true);
  assert.deepEqual(lifecycleDiff.comparison.unmatched_candidate_sequences, []);
  assert.deepEqual(lifecycleDiff.comparison.damage_identity_mismatches, []);
  assert.equal(Number(lifecycleDiff.comparison.trusted_rows), 39);
  assert.equal(Number(lifecycleDiff.comparison.old_suppressed_rows), 184);
  assert.equal(Number(lifecycleDiff.comparison.trusted_harmony_individually_emitted_rows), ledger.length);
  assert.equal(Number(lifecycleDiff.comparison.candidate_harmony_individually_emitted_rows), ledger.length);
  assert.equal(Number(lifecycleDiff.comparison.trusted_unresolved_status_confounder_damage_rows), 41_003);
  assert.equal(Number(lifecycleDiff.comparison.candidate_unresolved_status_confounder_damage_rows), 6_782);
  verifyEmbeddedArtifact(lifecycleDiff.inputs.candidate_ledger, auditPath);
  verifyEmbeddedArtifact(lifecycleDiff.inputs.rlog, path.resolve(replay.source_path));
  assert.equal(path.resolve(lifecycleDiffPath), path.resolve(fileReceipt(lifecycleDiffPath).path));

  const terminalAware = model(lifecycleDiff, "terminal_aware_exact_instance");
  assert.equal(Number(terminalAware.allowed_rows), ledger.length);
  assert.equal(Number(terminalAware.blocked_rows), 0);
  assert.equal(terminalAware.exact_match_to_trusted_ledger, false);
  const stickyEvery = model(lifecycleDiff, "sticky_every_observation");
  assert.equal(Number(stickyEvery.allowed_rows), 39);
  assert.equal(Number(stickyEvery.blocked_rows), 184);
  assert.equal(Number(stickyEvery.false_included), 0);
  assert.equal(Number(stickyEvery.false_suppressed), 0);
  assert.equal(stickyEvery.exact_match_to_trusted_ledger, true);

  assert.equal(protocolStatus.schema_version, 4);
  assert.equal(protocolStatus.generated_by, "tools/bpsr-protocol-pack-status.mjs");
  assert.equal(String(protocolStatus.game_build), build);
  assert.equal(protocolStatus.status, "promoted");
  assert.equal(protocolStatus.candidate.audited_digest, replay.protocol_pack_digest);
  assert.equal(protocolStatus.promoted_pack.present, true);
  assert.equal(protocolStatus.promoted_pack.build_matches, true);
  assert.equal(protocolStatus.promoted_pack.byte_identical_to_candidate, true);
  assert.equal(
    protocolStatus.policy.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements,
    true,
  );
  assert.equal(protocolStatus.policy.unknown_and_unresolved_canonical_events_are_preserved, true);

  assert.equal(segmentReport.schema_version, 1);
  assert.equal(String(segmentReport.requirements.expected_build), build);
  assert.equal(segmentReport.requirements.expected_protocol_pack_digest, replay.protocol_pack_digest);
  assert.equal(segmentReport.requirements.remote_player_cast_packet_required, false);
  const segment = segmentReport.segments.find((row) => row.session_id === replay.session_id);
  assert.ok(segment, `segmentation has no ${replay.session_id}`);
  assert.equal(segment.completed, true);
  assert.equal(Number(segment.event_count), Number(replay.event_count));

  const providers = unique(ledger.map((row) => String(row.provider_actor_id)));
  const recipients = unique(ledger.map((row) => String(row.recipient_actor_id)));
  const instances = unique(
    ledger.map((row) => String(requiredValue(row.formula_trace?.primary_transition_instance_id,
      `sequence ${row.sequence} formula trace instance`))),
  );
  assert.equal(providers.length, 1);
  assert.equal(recipients.length, 1);
  for (const row of ledger) {
    assert.equal(Number(row.effect_id), EFFECT_ID);
    assert.ok(row.formula_trace, `sequence ${row.sequence} has no formula trace`);
    assert.equal(Number(row.formula_trace.effect_id), EFFECT_ID);
    assert.equal(String(row.formula_trace.provider_actor_id), providers[0]);
    assert.equal(String(row.formula_trace.recipient_actor_id), recipients[0]);
    assert.equal(String(row.formula_trace.contribution_numerator), String(row.numerator));
    assert.equal(String(row.formula_trace.contribution_denominator), String(row.denominator));
  }

  const exact = sumFractions(ledger);
  const integerProjection = roundHalfUp(exact.numerator, exact.denominator);
  assert.equal(BigInt(replay.summary.attributed_bonus_damage), integerProjection);
  const effect = replay.summary.effects.find(
    (row) => Number(row.effect_id) === EFFECT_ID
      && String(row.provider_actor_id) === providers[0]
      && String(row.recipient_actor_id) === recipients[0],
  );
  assert.ok(effect, "replay summary omits Harmony Grace candidate transfer");
  assert.equal(BigInt(effect.amount), integerProjection);
  const observedDamage = ledger.reduce((sum, row) => sum + BigInt(row.observed_damage), 0n);
  const rawTotal = actorTotal(replay.summary.actors, "raw_damage");
  const rdpsTotal = actorTotal(replay.summary.actors, "rdps_damage");
  assert.equal(rawTotal, rdpsTotal);

  return {
    identity: {
      deployment_id: replay.deployment_id,
      session_id: replay.session_id,
      protocol_pack_digest: replay.protocol_pack_digest,
      provider_actor_ids: providers,
      recipient_actor_ids: recipients,
      lifecycle_instance_ids: instances,
    },
    proof: {
      exact_current_pack_identity: true,
      protocol_pack_promoted_for_decoding: true,
      packet_batched_closed_run: true,
      all_candidate_damage_identities_replayed: true,
      exact_formula_trace_for_every_candidate_row: true,
      historical_39_184_split_reproduced_by_legacy_sticky_terminal_model: true,
      terminal_aware_unresolved_lifecycle_has_zero_candidate_blockers: true,
      exact_rational_transfer_replayed: true,
      integer_projection_matches_replay_summary: true,
      ordinary_damage_conserved: true,
      remote_player_cast_packets_required: false,
    },
    summary: {
      closed_run_events: Number(segment.event_count),
      candidate_damage_rows: ledger.length,
      historical_subset_rows: Number(lifecycleDiff.comparison.trusted_rows),
      legacy_falsely_suppressed_rows: Number(lifecycleDiff.comparison.old_suppressed_rows),
      numeric_ability_count: unique(ledger.map((row) => String(row.affected_damage_id))).length,
      allegiance_neutral_target_count: unique(ledger.map((row) => String(row.target_actor_id))).length,
      observed_damage: observedDamage.toString(),
      exact_contribution: {
        numerator: exact.numerator.toString(),
        denominator: exact.denominator.toString(),
        decimal: decimal(exact.numerator, exact.denominator, 9),
      },
      integer_projected_contribution: integerProjection.toString(),
      ordinary_raw_damage: rawTotal.toString(),
      ordinary_rdps_damage: rdpsTotal.toString(),
      replay_runtime_target_match: false,
      replay_candidate_audit_target_match: true,
    },
  };
}

function verify(values) {
  const inputPath = path.resolve(required(values, "input"));
  const report = readJson(inputPath);
  validateReport(report);
  for (const descriptor of Object.values(report.inputs)) verifyReceipt(descriptor);
  const analysis = analyze({
    build: report.game_build,
    audit: readJson(report.inputs.replay_audit.path),
    auditPath: report.inputs.replay_audit.path,
    lifecycleDiff: readJson(report.inputs.lifecycle_differential.path),
    lifecycleDiffPath: report.inputs.lifecycle_differential.path,
    protocolStatus: readJson(report.inputs.protocol_status.path),
    segmentReport: readJson(report.inputs.run_segmentation.path),
  });
  assert.deepEqual(report.identity, analysis.identity);
  assert.deepEqual(report.proof, analysis.proof);
  assert.deepEqual(report.summary, analysis.summary);
  process.stdout.write(`${JSON.stringify(report.summary, null, 2)}\n`);
}

function validateReport(report) {
  assert.equal(report.schema_version, SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(Number(report.effect_id), EFFECT_ID);
  assert.equal(report.topology.target_allegiance_inferred, false);
  assert.equal(report.topology.remote_player_cast_packet_required, false);
  assert.equal(report.policy.provider_rdps_credit_allowed, false);
  assert.equal(report.policy.runtime_promotion_allowed, false);
  assert.equal(report.policy.ui_display_allowed, false);
  assert.equal(report.proof.ordinary_damage_conserved, true);
  assert.equal(report.summary.replay_runtime_target_match, false);
  assert.equal(
    report.decision,
    "exact-current-pack-lifecycle-regression-closed-formula-candidate-runtime-disabled",
  );
  assert.equal(report.content_sha256, contentHash(report));
}

function selfTest() {
  assert.deepEqual(
    sumFractions([{ numerator: "1", denominator: "3" }, { numerator: "1", denominator: "6" }]),
    { numerator: 1n, denominator: 2n },
  );
  assert.equal(roundHalfUp(7n, 2n), 4n);
  assert.equal(decimal(7n, 2n, 3), "3.500");
  process.stdout.write("self-test passed\n");
}

function model(report, name) {
  const value = report.unresolved_lifecycle_models.find((row) => row.model === name);
  assert.ok(value, `lifecycle differential omits ${name}`);
  return value;
}

function verifyEmbeddedArtifact(descriptor, filePath) {
  const current = fileReceipt(filePath);
  assert.equal(normalizeDigest(descriptor.sha256), current.sha256);
  assert.equal(Number(descriptor.bytes), current.bytes);
}

function normalizeDigest(value) {
  return String(value).replace(/^sha256:/i, "").toLowerCase();
}

function sumFractions(rows) {
  let numerator = 0n;
  let denominator = 1n;
  for (const row of rows) {
    numerator = numerator * BigInt(row.denominator) + BigInt(row.numerator) * denominator;
    denominator *= BigInt(row.denominator);
    const divisor = gcd(numerator, denominator);
    numerator /= divisor;
    denominator /= divisor;
  }
  return { numerator, denominator };
}

function roundHalfUp(numerator, denominator) {
  assert.ok(numerator >= 0n && denominator > 0n);
  return (numerator * 2n + denominator) / (denominator * 2n);
}

function decimal(numerator, denominator, digits) {
  const whole = numerator / denominator;
  const remainder = numerator % denominator;
  const scale = 10n ** BigInt(digits);
  const fraction = (remainder * scale / denominator).toString().padStart(digits, "0");
  return `${whole}.${fraction}`;
}

function gcd(left, right) {
  left = left < 0n ? -left : left;
  right = right < 0n ? -right : right;
  while (right !== 0n) [left, right] = [right, left % right];
  return left;
}

function actorTotal(actors, field) {
  return Object.values(actors).reduce((sum, actor) => sum + BigInt(actor[field]), 0n);
}

function unique(values) {
  return [...new Set(values)].sort();
}

function requiredValue(value, label) {
  assert.notEqual(value, null, `${label} is null`);
  assert.notEqual(value, undefined, `${label} is absent`);
  return value;
}

function fileReceipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return { path: path.resolve(filePath), bytes: bytes.length, sha256: sha256(bytes) };
}

function verifyReceipt(descriptor) {
  const current = fileReceipt(descriptor.path);
  assert.equal(current.bytes, descriptor.bytes);
  assert.equal(current.sha256, descriptor.sha256);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return sha256(Buffer.from(JSON.stringify(copy)));
}

function refuseExisting(filePath) {
  if (fs.existsSync(filePath) || fs.existsSync(`${filePath}.partial`)) {
    throw new Error(`refusing to overwrite ${filePath}`);
  }
}

function required(values, key) {
  if (!values[key]) throw new Error(`missing --${key}`);
  return values[key];
}

function parseArgs(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    if (!key?.startsWith("--") || index + 1 >= values.length) usage(1);
    parsed[key.slice(2)] = values[index + 1];
  }
  return parsed;
}

function usage(exitCode) {
  process.stderr.write(
    "usage:\n" +
      "  node tools/bpsr-harmony-grace-current-pack-lifecycle-closure.mjs generate --build <id> --audit <json> --lifecycle-diff <json> --protocol-status <json> --segment-report <json> --output <json>\n" +
      "  node tools/bpsr-harmony-grace-current-pack-lifecycle-closure.mjs verify --input <json>\n" +
      "  node tools/bpsr-harmony-grace-current-pack-lifecycle-closure.mjs self-test\n",
  );
  process.exit(exitCode);
}
