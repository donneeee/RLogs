#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 4;
const GENERATED_BY = "tools/bpsr-harmony-exact-rational-promotion-proof.mjs";
const EFFECT_ID = 3_003_052;
const RECIPIENT_CLASS_ID = 11;
const GAME_BUILD = "24687926";
const PROTOCOL_PACK_DIGEST =
  "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395";
const PROJECTION_POLICY = "sum-exact-then-half-up-per-effect-provider-recipient";
const MAX_INPUT_BYTES = 16 * 1024 * 1024;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") build(options);
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(args) {
  const inputs = resolveInputs(args);
  const output = path.resolve(required(args, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);
  const report = buildReport(inputs);
  report.content_sha256 = contentHash(report);
  validateReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(
    `Harmony exact-rational revocation receipt built: rows=${report.summary.accepted_damage_rows}, ` +
      `projected=${report.summary.projected_integer_contribution}, ` +
      `promotion_allowed=${report.decision.component_promotion_allowed}.`,
  );
}

function verify(input) {
  requireFile(input, "promotion proof");
  const report = readJson(input, "promotion proof");
  validateReport(report);
  const rebuilt = buildReport(resolveDescriptorInputs(report.inputs));
  assert(stableStringify(rebuilt) === stableStringify(withoutContentHash(report)),
    "Harmony revocation receipt does not reproduce");
  console.log(
    `Harmony exact-rational revocation receipt verified: projected=${report.summary.projected_integer_contribution}, ` +
      `promotion_allowed=${report.decision.component_promotion_allowed}.`,
  );
}

function resolveInputs(args) {
  const inputs = {
    capture_refresh_manifest: path.resolve(required(args, "manifest")),
    exact_rational_trace: path.resolve(required(args, "trace")),
    lifecycle_transition_proof: path.resolve(required(args, "transition-proof")),
    replay_audit: path.resolve(required(args, "replay-audit")),
    production_replay: path.resolve(required(args, "production-replay")),
    runtime: path.resolve(required(args, "runtime")),
  };
  for (const [label, input] of Object.entries(inputs)) requireFile(input, label);
  return inputs;
}

function resolveDescriptorInputs(inputs) {
  return Object.fromEntries(Object.entries(inputs).map(([key, value]) => {
    assert(typeof value?.path === "string", `Missing input path for ${key}`);
    return [key, path.resolve(value.path)];
  }));
}

function buildReport(inputPaths) {
  const manifest = readJson(inputPaths.capture_refresh_manifest, "capture refresh manifest");
  const trace = readJson(inputPaths.exact_rational_trace, "exact-rational trace");
  const transition = readJson(inputPaths.lifecycle_transition_proof, "lifecycle transition proof");
  const replay = readJson(inputPaths.replay_audit, "replay audit");
  const productionReplay = readJson(inputPaths.production_replay, "production replay");
  const runtime = readJson(inputPaths.runtime, "runtime");

  const candidateReplayReport = replay.reports?.[0];
  assert(candidateReplayReport && replay.reports.length === 1,
    "Candidate replay must contain exactly one report");
  const replayReport = productionReplay.reports?.[0];
  assert(replayReport && productionReplay.reports.length === 1,
    "Production replay must contain exactly one report");
  const terms = replayReport.summary?.rational_effects;
  assert(Array.isArray(terms) && terms.length > 0, "Replay has no exact rational terms");
  let exactNumerator = 0n;
  let exactDenominator = 1n;
  for (const term of terms) {
    assert(term.effect_id === EFFECT_ID, "Replay contains a different effect");
    assert(term.provider_actor_id === 4547 && term.recipient_actor_id === 13,
      "Replay contains a different provider/recipient edge");
    const numerator = positiveBigInt(term.numerator, "rational numerator");
    const denominator = positiveBigInt(term.denominator, "rational denominator");
    [exactNumerator, exactDenominator] = addFraction(
      exactNumerator,
      exactDenominator,
      numerator,
      denominator,
    );
  }
  const projected = roundHalfUp(exactNumerator, exactDenominator);
  const traceFraction = reduceFraction(
    positiveBigInt(trace.summary?.exact_contribution?.numerator, "trace numerator"),
    positiveBigInt(trace.summary?.exact_contribution?.denominator, "trace denominator"),
  );
  assert(traceFraction[0] === exactNumerator && traceFraction[1] === exactDenominator,
    "Trace and replay exact rational totals differ");

  const actors = replayReport.summary?.actors ?? {};
  const rawTotal = sumActorField(actors, "raw_damage");
  const rdpsTotal = sumActorField(actors, "rdps_damage");
  const provider = actors["4547"];
  const recipient = actors["13"];

  const descriptors = Object.fromEntries(
    Object.entries(inputPaths).map(([key, value]) => [key, descriptor(value)]),
  );
  assert(manifest.outputs?.trace?.sha256 === descriptors.exact_rational_trace.sha256,
    "Manifest trace digest does not match input");
  assert(manifest.outputs?.transitionProof?.sha256 === descriptors.lifecycle_transition_proof.sha256,
    "Manifest transition digest does not match input");
  assert(manifest.outputs?.audit?.sha256 === descriptors.replay_audit.sha256,
    "Manifest replay digest does not match input");

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    protocol_pack_digest: PROTOCOL_PACK_DIGEST,
    effect_id: EFFECT_ID,
    candidate_scope: {
      recipient_class_ids: [RECIPIENT_CLASS_ID],
      provider_actor_id: "4547",
      recipient_actor_id: "13",
      damage_scripts: ["Attack"],
      other_recipient_classes_promoted: false,
      rejected_or_unresolved_rows_promoted: false,
    },
    attribution_contract: {
      authoritative_input: "observed integer damage",
      exact_fraction:
        "observed_damage * provider_removed_attack_stage_body / active_attack_stage_body",
      integer_projection: PROJECTION_POLICY,
      projection_scope: "effect-provider-recipient encounter total",
      server_counterfactual_integer_boundary_claimed: false,
      server_counterfactual_integer_boundary_required_for_this_contract: false,
      packet_final_integer_is_authoritative: true,
      hidden_server_intermediate_is_not_required: true,
      provider_credit_equals_recipient_debit: true,
      ordinary_damage_is_mutated: false,
      unknown_or_unresolved_overlap_transfer: 0,
      canonical_unknown_and_unresolved_events_are_preserved: true,
      candidate_projection_status:
        "superseded-primary-to-attack-route-retained-for-revocation-audit-only",
      candidate_projection_value_must_not_be_displayed_as_rdps: true,
    },
    inputs: descriptors,
    evidence: {
      capture_identity_matches_runtime:
        manifest.game_build === runtime.game_build &&
        manifest.identity?.protocol_pack_digest === runtime.protocol_pack_digest,
      exact_provider_recipient_lifecycle:
        trace.proof?.exact_provider_recipient_lifecycle === true,
      exact_same_lifecycle_primary_transition:
        trace.proof?.exact_same_lifecycle_primary_transition_marginal === true,
      exact_primary_to_attack_replay:
        trace.proof?.exact_primary_to_attack_floor_replay === true,
      exact_attack_family_replay: trace.proof?.exact_attack_family_replay === true,
      exact_damage_stage_replay: trace.proof?.exact_damage_stage_replay === true,
      packet_witness_supported_damage_rows:
        transition.summary?.packet_witness_supported_damage_rows,
      unsupported_accepted_damage_rows: transition.summary?.unsupported_damage_rows,
      replay_conserved: replayReport.conserved === true,
      production_attribution_mode: productionReplay.attribution_mode,
      production_runtime_target_match: replayReport.runtime_target_match,
      production_candidate_audit_target_match: replayReport.candidate_audit_target_match,
      production_emitted_rows:
        replayReport.emitted_contribution_events_by_effect?.[String(EFFECT_ID)],
      candidate_and_production_transfer_match:
        candidateReplayReport.summary?.attributed_bonus_damage ===
          replayReport.summary?.attributed_bonus_damage &&
        candidateReplayReport.emitted_contribution_events_by_effect?.[String(EFFECT_ID)] ===
          replayReport.emitted_contribution_events_by_effect?.[String(EFFECT_ID)],
      rational_projection_overflow_count:
        replayReport.summary?.rational_projection_overflow_count,
      runtime_schema_version: runtime.schema_version,
      runtime_class_11_lifecycle_authority:
        runtime.harmony_grace?.class_11_current_pack_lifecycle_authority,
      runtime_class_11_exact_rational_authority:
        runtime.harmony_grace?.class_11_exact_rational_attribution_authority,
      runtime_server_integer_counterfactual_authority:
        runtime.harmony_grace?.server_integer_counterfactual_authority,
      runtime_unresolved_overlap_fails_closed:
        runtime.harmony_grace?.unresolved_overlap_fails_closed,
      runtime_projection_policy: runtime.harmony_grace?.rational_integer_projection,
      runtime_transfer_enabled: runtime.harmony_grace?.runtime_transfer_enabled,
      runtime_recipient_class_ids: runtime.harmony_grace?.runtime_recipient_class_ids,
    },
    summary: {
      accepted_damage_rows: trace.summary?.emitted_damage_rows,
      lifecycle_instances: trace.summary?.lifecycle_instances,
      observed_damage_in_accepted_rows: trace.summary?.observed_damage,
      exact_contribution_numerator: exactNumerator.toString(),
      exact_contribution_denominator: exactDenominator.toString(),
      projected_integer_contribution: projected.toString(),
      replay_attributed_bonus_damage:
        String(replayReport.summary?.attributed_bonus_damage),
      replay_raw_damage_total: rawTotal.toString(),
      replay_rdps_damage_total: rdpsTotal.toString(),
      provider_contribution_given: String(provider?.contribution_given),
      recipient_contribution_received: String(recipient?.contribution_received),
    },
    decision: {
      component_promotion_allowed: false,
      production_promotion_count_delta: 0,
      runtime_authority: false,
      ui_display_authority: false,
      display_qualifier: "diagnostic proportional candidate; no rDPS credit",
      global_runtime_promotion_allowed: runtime.policy?.runtime_promotion_allowed === true,
      unrelated_effects_promoted: false,
    },
    blocking_scope: [
      "the complete provider-sensitive damage-stage body and its operation order are not yet proven, so proportional cancellation remains diagnostic only",
      "recipient classes other than 11 remain unpromoted",
      "rows without exact lifecycle, packet transition, damage stage, or overlap closure remain uncredited",
      "action-opportunity effects such as speed remain uncredited",
    ],
  };
}

function validateReport(report) {
  assert(report.schema_version === SCHEMA_VERSION, "Wrong promotion proof schema");
  assert(report.generated_by === GENERATED_BY, "Wrong promotion proof generator");
  assert(report.game_build === GAME_BUILD, "Wrong game build");
  assert(report.protocol_pack_digest === PROTOCOL_PACK_DIGEST, "Wrong protocol pack");
  assert(report.effect_id === EFFECT_ID, "Wrong effect ID");
  assert(stableStringify(report.candidate_scope?.recipient_class_ids) === "[11]",
    "Candidate scope must remain class 11 only");
  assert(report.attribution_contract?.authoritative_input === "observed integer damage",
    "Observed integer damage is not authoritative");
  assert(report.attribution_contract?.integer_projection === PROJECTION_POLICY,
    "Wrong rational integer projection");
  assert(report.attribution_contract?.candidate_projection_status ===
    "superseded-primary-to-attack-route-retained-for-revocation-audit-only" &&
    report.attribution_contract?.candidate_projection_value_must_not_be_displayed_as_rdps === true,
  "Superseded Harmony projection regained display authority");
  assert(report.attribution_contract?.server_counterfactual_integer_boundary_claimed === false &&
    report.attribution_contract?.server_counterfactual_integer_boundary_required_for_this_contract === false &&
    report.attribution_contract?.packet_final_integer_is_authoritative === true &&
    report.attribution_contract?.hidden_server_intermediate_is_not_required === true,
  "Server counterfactual boundary was misrepresented");
  assert(report.attribution_contract?.provider_credit_equals_recipient_debit === true &&
    report.attribution_contract?.ordinary_damage_is_mutated === false &&
    report.attribution_contract?.unknown_or_unresolved_overlap_transfer === 0,
  "Conservation or fail-closed contract is missing");

  const evidence = report.evidence ?? {};
  for (const key of [
    "capture_identity_matches_runtime",
    "exact_provider_recipient_lifecycle",
    "exact_same_lifecycle_primary_transition",
    "exact_primary_to_attack_replay",
    "exact_attack_family_replay",
    "exact_damage_stage_replay",
    "replay_conserved",
    "runtime_class_11_lifecycle_authority",
    "runtime_unresolved_overlap_fails_closed",
    "production_runtime_target_match",
    "candidate_and_production_transfer_match",
  ]) assert(evidence[key] === true, `Required evidence is not true: ${key}`);
  assert(evidence.runtime_schema_version === 21, "Wrong runtime schema");
  assert(evidence.runtime_class_11_exact_rational_authority === false &&
    evidence.runtime_transfer_enabled === false,
  "Revoked Harmony proportional transfer regained runtime authority");
  assert(evidence.runtime_server_integer_counterfactual_authority === false,
    "Server integer authority must remain unclaimed");
  assert(evidence.runtime_projection_policy === PROJECTION_POLICY, "Runtime projection drifted");
  assert(stableStringify(evidence.runtime_recipient_class_ids) === "[]",
    "Runtime recipient scope must remain empty while the complete damage body is unresolved");
  assert(evidence.packet_witness_supported_damage_rows === 223 &&
    evidence.unsupported_accepted_damage_rows === 0,
  "Accepted damage rows are not fully packet-witness supported");
  assert(evidence.rational_projection_overflow_count === 0,
    "Exact rational projection overflowed");
  assert(evidence.production_attribution_mode === "production_promoted_rules" &&
    evidence.production_candidate_audit_target_match === false &&
    evidence.production_emitted_rows === 223,
  "Replay did not use only the promoted production rule");

  const summary = report.summary ?? {};
  assert(summary.accepted_damage_rows === 223, "Unexpected accepted row count");
  assert(summary.observed_damage_in_accepted_rows === "38436848", "Unexpected observed damage");
  assert(summary.projected_integer_contribution === "87606", "Unexpected integer projection");
  assert(summary.replay_attributed_bonus_damage === "87606", "Replay projection differs");
  assert(summary.replay_raw_damage_total === summary.replay_rdps_damage_total,
    "Ordinary and rDPS totals do not conserve");
  assert(summary.provider_contribution_given === "87606" &&
    summary.recipient_contribution_received === "87606",
  "Provider credit and recipient debit differ");
  assert(report.decision?.component_promotion_allowed === false &&
    report.decision?.production_promotion_count_delta === 0 &&
    report.decision?.runtime_authority === false &&
    report.decision?.ui_display_authority === false &&
    report.decision?.global_runtime_promotion_allowed === false &&
    report.decision?.unrelated_effects_promoted === false,
  "Promotion decision is inconsistent");
  assert(report.content_sha256 === contentHash(report), "Promotion proof digest mismatch");
}

function descriptor(input) {
  const info = statSync(input);
  assert(info.size <= MAX_INPUT_BYTES, `Input exceeds ${MAX_INPUT_BYTES} bytes: ${input}`);
  return {
    path: path.resolve(input),
    bytes: info.size,
    sha256: createHash("sha256").update(readFileSync(input)).digest("hex"),
  };
}

function readJson(input, label) {
  const info = statSync(input);
  assert(info.size <= MAX_INPUT_BYTES, `${label} exceeds ${MAX_INPUT_BYTES} bytes`);
  return JSON.parse(readFileSync(input, "utf8"));
}

function sumActorField(actors, field) {
  return Object.values(actors).reduce(
    (total, actor) => total + BigInt(actor?.[field] ?? 0),
    0n,
  );
}

function positiveBigInt(value, label) {
  const parsed = BigInt(value);
  assert(parsed > 0n, `${label} must be positive`);
  return parsed;
}

function addFraction(leftNumerator, leftDenominator, rightNumerator, rightDenominator) {
  const common = gcd(leftDenominator, rightDenominator);
  return reduceFraction(
    leftNumerator * (rightDenominator / common) +
      rightNumerator * (leftDenominator / common),
    leftDenominator * (rightDenominator / common),
  );
}

function reduceFraction(numerator, denominator) {
  const divisor = gcd(numerator, denominator);
  return [numerator / divisor, denominator / divisor];
}

function roundHalfUp(numerator, denominator) {
  return (numerator * 2n + denominator) / (denominator * 2n);
}

function gcd(left, right) {
  left = left < 0n ? -left : left;
  right = right < 0n ? -right : right;
  while (right !== 0n) [left, right] = [right, left % right];
  return left || 1n;
}

function contentHash(report) {
  return createHash("sha256").update(stableStringify(withoutContentHash(report))).digest("hex");
}

function withoutContentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return copy;
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function requireFile(input, label) {
  assert(existsSync(input) && statSync(input).isFile(), `Missing ${label}: ${input}`);
}

function required(args, key) {
  const value = args[key];
  assert(typeof value === "string" && value.length > 0, `Missing --${key}`);
  return value;
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    assert(key?.startsWith("--") && value !== undefined, `Invalid argument: ${key ?? ""}`);
    parsed[key.slice(2)] = value;
  }
  return parsed;
}

function selfTest() {
  assert(stableStringify(addFraction(1n, 3n, 1n, 6n).map(String)) === '["1","2"]',
    "Exact addition self-test failed");
  assert(roundHalfUp(1n, 2n) === 1n && roundHalfUp(49n, 100n) === 0n,
    "Half-up self-test failed");
  console.log("Harmony exact-rational revocation receipt self-test passed.");
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(exitCode) {
  console.log(
    "Usage:\n" +
      "  node tools/bpsr-harmony-exact-rational-promotion-proof.mjs build " +
      "--manifest <json> --trace <json> --transition-proof <json> " +
      "--replay-audit <json> --production-replay <json> --runtime <json> --output <json>\n" +
      "  node tools/bpsr-harmony-exact-rational-promotion-proof.mjs verify --input <json>\n" +
      "  node tools/bpsr-harmony-exact-rational-promotion-proof.mjs self-test",
  );
  process.exit(exitCode);
}
