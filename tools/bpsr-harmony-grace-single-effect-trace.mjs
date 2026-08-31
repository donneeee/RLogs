#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-harmony-grace-single-effect-trace.mjs";
const SCHEMA_VERSION = 6;
const MINIMUM_REPLAY_SCHEMA_VERSION = 23;
const EFFECT_ID = 3_003_052;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(values) {
  const auditPath = path.resolve(required(values, "audit"));
  const lifecyclePath = path.resolve(required(values, "lifecycle-events"));
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const audit = readJson(auditPath);
  validateAudit(audit);
  const report = audit.reports[0];
  const lifecycle = readJsonl(lifecyclePath)
    .map(statusRow)
    .filter(Boolean)
    .sort((left, right) => left.sequence - right.sequence);
  const traces = report.emitted_contribution_ledger
    .filter((entry) => Number(entry.effect_id) === EFFECT_ID)
    .map((entry) => buildTrace(entry, lifecycle));
  if (traces.length === 0) throw new Error("audit contains no emitted Harmony Grace rows");
  const exactContribution = sumContributions(traces);
  const roundingEvidence = summarizeFamilyRounding(report.harmony_grace_family_rounding_diagnostics);

  const output = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    effect_id: EFFECT_ID,
    deployment_id: report.deployment_id,
    game_build: report.client_build,
    protocol_pack_digest: report.protocol_pack_digest,
    session_id: report.session_id,
    source: {
      audit: fileReceipt(auditPath),
      lifecycle_events: fileReceipt(lifecyclePath),
    },
    policy: {
      scope: "one_effect_end_to_end_observed_row_proof",
      current_character_snapshots_used: false,
      unresolved_rows_synthesized: false,
      production_runtime_changed: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
    proof: {
      localized_description_evidence: "Adaptive main stats +2%",
      authoritative_effect_id: EFFECT_ID,
      exact_provider_recipient_lifecycle: true,
      exact_provider_raw_percent: 200,
      exact_same_lifecycle_primary_transition_marginal: true,
      exact_primary_family_replay: true,
      exact_primary_to_attack_floor_replay: true,
      exact_attack_family_replay: true,
      exact_damage_stage_replay: true,
      replay_conserved: report.conserved === true,
      formula_trace_proven_for_observed_rows: true,
      rejected_rounding_states_retained: true,
      primary_family_rounding_evidence: roundingEvidence,
    },
    summary: {
      emitted_damage_rows: traces.length,
      providers: [...new Set(traces.map((trace) => trace.provider_actor_id))],
      recipients: [...new Set(traces.map((trace) => trace.recipient_actor_id))],
      recipient_classes: [...new Set(traces.map((trace) => trace.arithmetic.recipient_class_id))],
      target_actors: [...new Set(traces.map((trace) => trace.damage_target_actor_id))],
      abilities: [...new Set(traces.map((trace) => trace.ability_id))],
      lifecycle_instances: [...new Set(traces.map((trace) => trace.lifecycle.instance_id))],
      observed_damage: traces.reduce((sum, trace) => sum + BigInt(trace.observed_damage), 0n).toString(),
      exact_contribution: {
        numerator: exactContribution.numerator.toString(),
        denominator: exactContribution.denominator.toString(),
        decimal: fractionDecimal(exactContribution.numerator, exactContribution.denominator, 6),
      },
      contribution_terms: traces.map((trace) => trace.contribution),
    },
    traces,
    conclusion: {
      one_effect_chain_reconstructed: true,
      reusable_generalization_allowed: false,
      remaining_before_runtime_promotion: [
        "repeat the exact trace across additional captures and recipient classes",
        "close the exact protocol-pack migration identity for the replayed capture",
        "close the required canonical-replay conservation and protocol-event-coverage gates for the exact pack",
        "prove overlap and stacking behavior for the selected build",
      ],
    },
  };
  output.content_sha256 = contentHash(output);
  verifyReport(output);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify(consoleSummary(output.summary), null, 2)}\n`);
}

function buildTrace(entry, lifecycle) {
  const formula = entry.formula_trace;
  if (!formula || Number(formula.effect_id) !== EFFECT_ID) {
    throw new Error(`sequence ${entry.sequence} has no Harmony Grace formula trace`);
  }
  verifyArithmetic(formula);
  const provider = String(formula.provider_actor_id);
  const recipient = String(formula.recipient_actor_id);
  const active = activeLifecycleAt(lifecycle, provider, recipient, Number(entry.sequence));
  if (!active) throw new Error(`sequence ${entry.sequence} has no active exact lifecycle`);
  if (String(formula.primary_transition_instance_id) !== active.instance_id) {
    throw new Error(`sequence ${entry.sequence} primary witness belongs to another lifecycle instance`);
  }
  return {
    damage_sequence: Number(entry.sequence),
    damage_capture_sequence: entry.capture_sequence ?? null,
    observed_micros: Number(entry.observed_micros),
    provider_actor_id: provider,
    provider_entity_uuid: entry.provider_entity_uuid,
    recipient_actor_id: recipient,
    recipient_entity_uuid: entry.recipient_entity_uuid,
    damage_source_actor_id: entry.damage_source_actor_id,
    damage_target_actor_id: entry.target_actor_id,
    damage_target_entity_uuid: entry.target_entity_uuid,
    ability_id: Number(formula.ability_id),
    damage_attr_id: Number(formula.damage_attr_id),
    observed_damage: String(formula.observed_damage),
    lifecycle: active,
    arithmetic: formula,
    contribution: {
      numerator: String(formula.contribution_numerator),
      denominator: String(formula.contribution_denominator),
      decimal: fractionDecimal(
        BigInt(formula.contribution_numerator),
        BigInt(formula.contribution_denominator),
        6,
      ),
    },
  };
}

function verifyArithmetic(trace) {
  const value = (key) => BigInt(trace[key]);
  const floorProduct = (left, right, scale = 10_000n) => (left * right) / scale;
  assert.equal(trace.primary_provider_marginal_basis, "same_lifecycle_packet_transition");
  assert.ok(Number.isSafeInteger(trace.primary_transition_capture_sequence));
  assert.ok(Number.isSafeInteger(trace.primary_transition_connection_id));
  assert.ok(Number.isSafeInteger(trace.primary_transition_stream_id));
  assert.notEqual(trace.primary_transition_instance_id, null);
  assert.equal(
    value("primary_intermediate") + value("primary_extra_add"),
    value("primary_final"),
  );
  assert.equal(value("provider_primary_raw_percent"), 200n);
  assert.ok(value("primary_provider_marginal") > 0n);
  const primaryWithout = value("primary_final") - value("primary_provider_marginal");
  assert.equal(primaryWithout, value("primary_without_provider"));
  assert.equal(
    value("primary_final") - primaryWithout,
    value("primary_provider_marginal"),
  );
  assert.equal(
    value("primary_final") * value("primary_to_attack_numerator") /
      value("primary_to_attack_denominator"),
    value("attack_component_with_provider"),
  );
  assert.equal(
    primaryWithout * value("primary_to_attack_numerator") /
      value("primary_to_attack_denominator"),
    value("attack_component_without_provider"),
  );
  assert.equal(
    value("attack_component_with_provider") - value("attack_component_without_provider"),
    value("provider_attack_base_add"),
  );
  assert.equal(
    floorProduct(value("attack_base_add"), 10_000n + value("attack_raw_percent")),
    value("attack_intermediate"),
  );
  assert.equal(
    value("attack_intermediate") + value("attack_extra_add"),
    value("attack_final"),
  );
  const attackWithout =
    floorProduct(
      value("attack_base_add") - value("provider_attack_base_add"),
      10_000n + value("attack_raw_percent"),
    ) + value("attack_extra_add");
  assert.equal(attackWithout, value("attack_without_provider"));
  assert.equal(value("attack_final") - attackWithout, value("provider_attack_marginal"));
  assert.equal(
    floorProduct(value("attack_final"), value("coefficient_basis_points")),
    value("active_coefficient_term"),
  );
  assert.equal(
    value("active_coefficient_term") + value("fixed_parameter"),
    value("active_stage_body"),
  );
  assert.equal(
    floorProduct(attackWithout, value("coefficient_basis_points")),
    value("without_provider_coefficient_term"),
  );
  assert.equal(
    value("active_coefficient_term") - value("without_provider_coefficient_term"),
    value("coefficient_stage_marginal"),
  );
  const numerator = value("observed_damage") * value("coefficient_stage_marginal");
  const denominator = value("active_stage_body");
  const divisor = gcd(numerator, denominator);
  assert.equal(numerator / divisor, value("contribution_numerator"));
  assert.equal(denominator / divisor, value("contribution_denominator"));
}

function activeLifecycleAt(rows, provider, recipient, sequence) {
  let active = null;
  for (const row of rows) {
    if (row.sequence > sequence) break;
    if (row.provider_actor_id !== provider || row.recipient_actor_id !== recipient) continue;
    if (["applied", "refreshed", "stacked"].includes(row.state)) active = row;
    else if (["removed", "consumed"].includes(row.state)) active = null;
  }
  if (!active) return null;
  const terminal = rows.find(
    (row) =>
      row.sequence > sequence &&
      row.provider_actor_id === provider &&
      row.recipient_actor_id === recipient &&
      row.instance_id === active.instance_id &&
      ["removed", "consumed"].includes(row.state),
  );
  return { ...active, terminal: terminal ?? null };
}

function statusRow(row) {
  const kind = row?.event?.data?.kind;
  const status = kind?.event === "status" ? kind.data : null;
  if (!status || Number(status.effect) !== EFFECT_ID) return null;
  return {
    sequence: Number(row.sequence),
    capture_sequence: row?.provenance?.source?.capture_sequence ?? null,
    observed_micros: Number(row?.time?.observed_micros),
    provider_actor_id: String(status.source?.actor_id ?? ""),
    provider_entity_uuid: status.source?.entity_uuid == null ? null : String(status.source.entity_uuid),
    recipient_actor_id: String(status.target.actor_id),
    recipient_entity_uuid: String(status.target.entity_uuid),
    instance_id: status.instance_id == null ? null : String(status.instance_id),
    source_type_id: status.origin?.source_type_id ?? null,
    source_config_id: status.origin?.source_config_id ?? null,
    state: status.state,
    stacks: status.stacks,
    duration_millis: status.duration_millis,
  };
}

function validateAudit(audit) {
  if (
    Number(audit?.schema_version) < MINIMUM_REPLAY_SCHEMA_VERSION ||
    audit?.attribution_mode !== "offline_candidate_gate_audit_not_production_attribution" ||
    audit?.harmony_grace_candidate_audit_enabled !== true ||
    !Array.isArray(audit?.reports) ||
    audit.reports.length !== 1 ||
    audit.reports[0]?.candidate_audit_target_match !== true ||
    audit.reports[0]?.conserved !== true
  ) {
    throw new Error("unsafe or incompatible Harmony Grace replay audit");
  }
}

function verify(values) {
  const report = readJson(path.resolve(required(values, "input")));
  verifyReport(report);
  process.stdout.write(`${JSON.stringify(consoleSummary(report.summary), null, 2)}\n`);
}

function consoleSummary(summary) {
  const { contribution_terms: _contributionTerms, ...concise } = summary;
  return concise;
}

function verifyReport(report) {
  assert.equal(report.schema_version, SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.effect_id, EFFECT_ID);
  assert.equal(report.policy.provider_rdps_credit_allowed, false);
  assert.equal(report.policy.runtime_promotion_allowed, false);
  assert.equal(report.proof.formula_trace_proven_for_observed_rows, true);
  assert.equal(report.proof.exact_same_lifecycle_primary_transition_marginal, true);
  assert.equal(report.proof.replay_conserved, true);
  assert.equal(report.proof.rejected_rounding_states_retained, true);
  const matchedPrimary = report.proof.primary_family_rounding_evidence.provider_decomposition_matched.primary;
  assert.ok(matchedPrimary.nearest_only_rows > 0);
  assert.ok(report.traces.length > 0);
  for (const trace of report.traces) {
    assert.equal(trace.lifecycle.provider_actor_id, trace.provider_actor_id);
    assert.equal(trace.lifecycle.recipient_actor_id, trace.recipient_actor_id);
    assert.ok(trace.lifecycle.sequence < trace.damage_sequence);
    assert.ok(trace.lifecycle.terminal.sequence > trace.damage_sequence);
    assert.equal(String(trace.arithmetic.primary_transition_instance_id), trace.lifecycle.instance_id);
    verifyArithmetic(trace.arithmetic);
  }
  const exactContribution = sumContributions(report.traces);
  assert.equal(report.summary.exact_contribution.numerator, exactContribution.numerator.toString());
  assert.equal(report.summary.exact_contribution.denominator, exactContribution.denominator.toString());
  assert.equal(report.content_sha256, contentHash(report));
}

function selfTest() {
  assert.equal(fractionDecimal(1n, 8n, 6), "0.125000");
  assert.equal(gcd(95_180n, 1_906n), 2n);
  process.stdout.write("self-test passed\n");
}

function fractionDecimal(numerator, denominator, digits) {
  const scale = 10n ** BigInt(digits);
  const scaled = numerator * scale / denominator;
  return `${scaled / scale}.${String(scaled % scale).padStart(digits, "0")}`;
}

function gcd(left, right) {
  left = left < 0n ? -left : left;
  right = right < 0n ? -right : right;
  while (right !== 0n) [left, right] = [right, left % right];
  return left;
}

function sumContributions(traces) {
  let numerator = 0n;
  let denominator = 1n;
  for (const trace of traces) {
    const termNumerator = BigInt(trace.contribution.numerator);
    const termDenominator = BigInt(trace.contribution.denominator);
    numerator = numerator * termDenominator + termNumerator * denominator;
    denominator *= termDenominator;
    const divisor = gcd(numerator, denominator);
    numerator /= divisor;
    denominator /= divisor;
  }
  return { numerator, denominator };
}

function summarizeFamilyRounding(rows) {
  if (!Array.isArray(rows) || rows.length === 0) throw new Error("missing Harmony Grace rounding diagnostics");
  const create = () => ({
    distinct_states: 0,
    damage_rows: 0,
    primary: { both_rows: 0, floor_only_rows: 0, nearest_only_rows: 0, neither_rows: 0 },
    attack: { both_rows: 0, floor_only_rows: 0, nearest_only_rows: 0, neither_rows: 0 },
  });
  const summary = { all_external_window_states: create(), provider_decomposition_matched: create() };
  for (const row of rows) {
    const damageRows = Number(row.damage_rows);
    const diagnostic = row.diagnostic;
    addRoundingState(summary.all_external_window_states, diagnostic, damageRows);
    if (diagnostic.provider_decomposition_matches === true) {
      addRoundingState(summary.provider_decomposition_matched, diagnostic, damageRows);
    }
  }
  return summary;
}

function addRoundingState(bucket, diagnostic, damageRows) {
  bucket.distinct_states += 1;
  bucket.damage_rows += damageRows;
  for (const family of ["primary", "attack"]) {
    const observedIntermediate = diagnostic[`${family}_observed_intermediate`];
    const observedFinal = diagnostic[`${family}_observed_final`];
    const floor = observedIntermediate === diagnostic[`${family}_floor_intermediate`]
      && observedFinal === diagnostic[`${family}_floor_final`];
    const nearest = observedIntermediate === diagnostic[`${family}_nearest_intermediate`]
      && observedFinal === diagnostic[`${family}_nearest_final`];
    const category = floor && nearest ? "both_rows" : floor ? "floor_only_rows" : nearest ? "nearest_only_rows" : "neither_rows";
    bucket[family][category] += damageRows;
  }
}

function fileReceipt(filePath) {
  const content = fs.readFileSync(filePath);
  return { path: filePath, bytes: content.length, sha256: crypto.createHash("sha256").update(content).digest("hex") };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function readJsonl(filePath) {
  return fs.readFileSync(filePath, "utf8").split(/\r?\n/).filter(Boolean).map(JSON.parse);
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return crypto.createHash("sha256").update(JSON.stringify(clone)).digest("hex");
}

function refuseExisting(filePath) {
  if (fs.existsSync(filePath)) throw new Error(`refusing to overwrite ${filePath}`);
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
    "usage: bpsr-harmony-grace-single-effect-trace.mjs generate --audit <schema23.json> --lifecycle-events <events.jsonl> --output <receipt.json>\n" +
      "       bpsr-harmony-grace-single-effect-trace.mjs verify --input <receipt.json>\n" +
      "       bpsr-harmony-grace-single-effect-trace.mjs self-test\n",
  );
  process.exit(exitCode);
}
