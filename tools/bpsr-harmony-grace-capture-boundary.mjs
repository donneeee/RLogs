#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-harmony-grace-capture-boundary.mjs";
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
  const tracePath = path.resolve(required(values, "trace"));
  const auditPath = path.resolve(required(values, "audit"));
  const protocolStatusPath = path.resolve(required(values, "protocol-status"));
  const segmentReportPath = path.resolve(required(values, "segment-report"));
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const inputs = {
    trace: readJson(tracePath),
    audit: readJson(auditPath),
    protocolStatus: readJson(protocolStatusPath),
    segmentReport: readJson(segmentReportPath),
  };
  const analysis = analyze({ build, ...inputs });
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: build,
    effect_id: EFFECT_ID,
    identity: analysis.identity,
    inputs: {
      exact_single_effect_trace: fileReceipt(tracePath),
      replay_audit: fileReceipt(auditPath),
      protocol_status: fileReceipt(protocolStatusPath),
      run_segmentation: fileReceipt(segmentReportPath),
    },
    topology: {
      support_path: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_path: "recipient damage action -> recipient or enemy target",
      target_allegiance_inferred: false,
      remote_player_cast_packet_required: false,
    },
    policy: {
      exact_numeric_ids_build_and_protocol_pack_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      unknown_or_unresolved_events_synthesized: false,
      current_character_snapshot_substitution_allowed: false,
      packet_state_counterfactual_is_server_integer_observation: false,
      ordinary_damage_mutation_allowed: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
    },
    proof: analysis.proof,
    summary: analysis.summary,
    blockers: [
      ...(analysis.proof.packet_batched_closed_run
        ? []
        : ["the selected packet-batched segment ended without the protocol Completed marker and is diagnostic-only"]),
      "a replicated exact absent/present/absent sequence must select the final server integer boundary",
      "recipient classes, providers, same-effect overlaps, and stacking combinations require explicit exact-build allowlists",
      "aggregate current-build replay and promotion suites remain independent gates",
    ],
    decision: analysis.proof.packet_batched_closed_run
      ? "exact-current-pack-capture-boundary-proven-formula-candidate-runtime-disabled"
      : "exact-current-pack-incomplete-capture-preserved-diagnostic-only-runtime-disabled",
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify(report.summary, null, 2)}\n`);
}

function analyze({ build, trace, audit, protocolStatus, segmentReport }) {
  assert.match(build, /^\d+$/);
  assert.equal(trace.schema_version, 6);
  assert.equal(trace.generated_by, "tools/bpsr-harmony-grace-single-effect-trace.mjs");
  assert.equal(String(trace.game_build), build);
  assert.equal(Number(trace.effect_id), EFFECT_ID);
  assert.equal(trace.proof.exact_provider_recipient_lifecycle, true);
  assert.equal(trace.proof.formula_trace_proven_for_observed_rows, true);
  assert.equal(trace.proof.replay_conserved, true);
  assert.ok(Array.isArray(trace.traces) && trace.traces.length > 0);

  assert.ok(Number(audit.schema_version) >= 23);
  assert.equal(audit.attribution_mode, "offline_candidate_gate_audit_not_production_attribution");
  assert.equal(audit.harmony_grace_candidate_audit_enabled, true);
  assert.ok(Array.isArray(audit.reports) && audit.reports.length === 1);
  const replay = audit.reports[0];
  assert.equal(String(replay.client_build), build);
  assert.equal(replay.session_id, trace.session_id);
  assert.equal(replay.protocol_pack_digest, trace.protocol_pack_digest);
  assert.equal(replay.candidate_audit_target_match, true);
  assert.equal(replay.conserved, true);
  assert.ok(Array.isArray(replay.emitted_contribution_ledger));
  const ledger = replay.emitted_contribution_ledger.filter(
    (row) => Number(row.effect_id) === EFFECT_ID,
  );
  assert.equal(ledger.length, trace.traces.length);
  const ledgerBySequence = new Map(ledger.map((row) => [Number(row.sequence), row]));
  for (const row of trace.traces) {
    const matched = ledgerBySequence.get(Number(row.damage_sequence));
    assert.ok(matched, `trace sequence ${row.damage_sequence} is absent from replay ledger`);
    assert.equal(String(matched.provider_actor_id), String(row.provider_actor_id));
    assert.equal(String(matched.recipient_actor_id), String(row.recipient_actor_id));
    assert.equal(String(matched.target_actor_id), String(row.damage_target_actor_id));
    assert.equal(String(matched.numerator), String(row.contribution.numerator));
    assert.equal(String(matched.denominator), String(row.contribution.denominator));
  }

  assert.equal(protocolStatus.schema_version, 4);
  assert.equal(protocolStatus.generated_by, "tools/bpsr-protocol-pack-status.mjs");
  assert.equal(String(protocolStatus.game_build), build);
  assert.equal(protocolStatus.status, "promoted");
  assert.equal(protocolStatus.candidate.audited_digest, trace.protocol_pack_digest);
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
  assert.equal(
    segmentReport.requirements.expected_protocol_pack_digest,
    trace.protocol_pack_digest,
  );
  assert.equal(segmentReport.requirements.remote_player_cast_packet_required, false);
  const segment = segmentReport.segments.find((row) => row.session_id === trace.session_id);
  assert.ok(segment, `segmentation has no ${trace.session_id}`);
  const segmentCompleted = segment.completed === true;
  assert.equal(Number(segment.event_count), Number(replay.event_count));

  const providers = unique(trace.traces.map((row) => String(row.provider_actor_id)));
  const recipients = unique(trace.traces.map((row) => String(row.recipient_actor_id)));
  const lifecycleInstances = unique(
    trace.traces.map((row) => String(row.lifecycle.instance_id)),
  );
  assert.ok(providers.length > 0 && recipients.length > 0 && lifecycleInstances.length > 0);
  const rawDamage = actorTotal(replay.summary.actors, "raw_damage");
  const rdpsDamage = actorTotal(replay.summary.actors, "rdps_damage");
  assert.equal(rawDamage, rdpsDamage);

  return {
    identity: {
      deployment_id: replay.deployment_id,
      session_id: trace.session_id,
      protocol_pack_digest: trace.protocol_pack_digest,
      provider_actor_ids: providers,
      recipient_actor_ids: recipients,
      lifecycle_instance_ids: lifecycleInstances,
    },
    proof: {
      exact_current_pack_identity: true,
      protocol_pack_promoted_for_decoding: true,
      packet_batched_closed_run: segmentCompleted,
      exact_provider_recipient_lifecycle: true,
      exact_formula_trace_for_every_candidate_row: true,
      all_trace_rows_match_replay_ledger: true,
      ordinary_damage_conserved: true,
      unknown_and_unresolved_events_preserved_by_decoder_policy: true,
      remote_player_cast_packets_required: false,
    },
    summary: {
      segmented_run_events: Number(segment.event_count),
      closed_run_events: segmentCompleted ? Number(segment.event_count) : null,
      segment_completed: segmentCompleted,
      segment_end_reason: segment.end_reason ?? null,
      candidate_damage_rows: trace.traces.length,
      providers: providers.length,
      recipients: recipients.length,
      lifecycle_instances: lifecycleInstances.length,
      numeric_abilities: unique(trace.traces.map((row) => String(row.ability_id))).length,
      allegiance_neutral_targets: unique(
        trace.traces.map((row) => String(row.damage_target_actor_id)),
      ).length,
      observed_damage: trace.traces.reduce(
        (sum, row) => sum + BigInt(row.observed_damage),
        0n,
      ).toString(),
      ordinary_raw_damage: rawDamage.toString(),
      ordinary_rdps_damage: rdpsDamage.toString(),
      replay_runtime_target_match: replay.runtime_target_match,
      replay_candidate_audit_target_match: replay.candidate_audit_target_match,
    },
  };
}

function verify(values) {
  const inputPath = path.resolve(required(values, "input"));
  const report = readJson(inputPath);
  validateReport(report);
  for (const receipt of Object.values(report.inputs)) verifyReceipt(receipt);
  const analysis = analyze({
    build: report.game_build,
    trace: readJson(report.inputs.exact_single_effect_trace.path),
    audit: readJson(report.inputs.replay_audit.path),
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
  assert.equal(report.proof.exact_current_pack_identity, true);
  assert.equal(typeof report.proof.packet_batched_closed_run, "boolean");
  assert.equal(report.summary.segment_completed, report.proof.packet_batched_closed_run);
  if (!report.proof.packet_batched_closed_run) {
    assert.equal(report.summary.closed_run_events, null);
  }
  assert.equal(report.proof.ordinary_damage_conserved, true);
  assert.equal(report.policy.provider_rdps_credit_allowed, false);
  assert.equal(report.policy.runtime_promotion_allowed, false);
  assert.equal(report.policy.ui_display_allowed, false);
  assert.equal(report.content_sha256, contentHash(report));
}

function actorTotal(actors, key) {
  return Object.values(actors ?? {}).reduce(
    (sum, actor) => sum + BigInt(actor[key] ?? 0),
    0n,
  );
}

function unique(values) {
  return [...new Set(values)].sort();
}

function selfTest() {
  assert.deepEqual(unique(["2", "1", "2"]), ["1", "2"]);
  assert.equal(actorTotal({ a: { raw_damage: 7 }, b: { raw_damage: "5" } }, "raw_damage"), 12n);
  process.stdout.write("self-test passed\n");
}

function fileReceipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: path.resolve(filePath),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function verifyReceipt(receipt) {
  assert.deepEqual(fileReceipt(receipt.path), receipt);
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(stableStringify(copy)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map(
      (key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function refuseExisting(filePath) {
  if (fs.existsSync(filePath)) throw new Error(`refusing to overwrite ${filePath}`);
}

function required(values, key) {
  const value = values.get(key);
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function parseArgs(values) {
  const parsed = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    if (!flag?.startsWith("--") || value === undefined) usage(1);
    parsed.set(flag.slice(2), value);
  }
  return parsed;
}

function usage(exitCode) {
  process.stderr.write(
    "usage:\n" +
      "  node tools/bpsr-harmony-grace-capture-boundary.mjs generate --build ID --trace FILE --audit FILE --protocol-status FILE --segment-report FILE --output FILE\n" +
      "  node tools/bpsr-harmony-grace-capture-boundary.mjs verify --input FILE\n" +
      "  node tools/bpsr-harmony-grace-capture-boundary.mjs self-test\n",
  );
  process.exit(exitCode);
}
