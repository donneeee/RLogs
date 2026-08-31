#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 4;
const GENERATED_BY = "tools/bpsr-mechanical-power-exact-rational-promotion-proof.mjs";
const EFFECT_ID = 2_110_140;
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
    `Mechanical Power exact-rational revocation receipt built: rows=${report.summary.accepted_damage_rows}, ` +
      `projected=${report.summary.projected_integer_contribution}, ` +
      `promotion_allowed=${report.decision.component_promotion_allowed}.`,
  );
}

function verify(input) {
  requireFile(input, "promotion proof");
  const report = readJson(input, "promotion proof");
  validateReport(report);
  const rebuilt = buildReport(resolveDescriptorInputs(report.inputs));
  assert(
    stableStringify(rebuilt) === stableStringify(withoutContentHash(report)),
    "Mechanical Power revocation receipt does not reproduce",
  );
  console.log(
    `Mechanical Power exact-rational revocation receipt verified: ` +
      `projected=${report.summary.projected_integer_contribution}, ` +
      `promotion_allowed=${report.decision.component_promotion_allowed}.`,
  );
}

function resolveInputs(args) {
  const inputs = {
    current_pack_replay_equivalence: path.resolve(required(args, "equivalence")),
    candidate_replay: path.resolve(required(args, "candidate-replay")),
    production_replay: path.resolve(required(args, "production-replay")),
    runtime: path.resolve(required(args, "runtime")),
  };
  for (const [label, input] of Object.entries(inputs)) requireFile(input, label);
  return inputs;
}

function resolveDescriptorInputs(inputs) {
  return Object.fromEntries(
    Object.entries(inputs).map(([key, value]) => {
      assert(typeof value?.path === "string", `Missing input path for ${key}`);
      return [key, path.resolve(value.path)];
    }),
  );
}

function buildReport(inputPaths) {
  const equivalence = readJson(
    inputPaths.current_pack_replay_equivalence,
    "current-pack replay equivalence",
  );
  const candidateReplay = readJson(inputPaths.candidate_replay, "candidate replay");
  const productionReplay = readJson(inputPaths.production_replay, "production replay");
  const runtime = readJson(inputPaths.runtime, "runtime");
  const candidate = onlyReport(candidateReplay, "candidate replay");
  const production = onlyReport(productionReplay, "production replay");
  const terms = production.summary?.rational_effects;
  assert(Array.isArray(terms) && terms.length > 0, "Production replay has no rational terms");

  let exactNumerator = 0n;
  let exactDenominator = 1n;
  for (const term of terms) {
    assert(term.effect_id === EFFECT_ID, "Production replay contains a different effect");
    assert(
      term.provider_actor_id === 5 && term.recipient_actor_id === 7,
      "Production replay contains a different provider/recipient edge",
    );
    [exactNumerator, exactDenominator] = addFraction(
      exactNumerator,
      exactDenominator,
      positiveBigInt(term.numerator, "rational numerator"),
      positiveBigInt(term.denominator, "rational denominator"),
    );
  }
  const projected = roundHalfUp(exactNumerator, exactDenominator);
  const candidateLedger = candidate.emitted_contribution_ledger;
  const productionLedger = production.emitted_contribution_ledger;
  assert(Array.isArray(candidateLedger), "Candidate replay has no emitted ledger");
  assert(Array.isArray(productionLedger), "Production replay has no emitted ledger");
  const candidateLedgerHash = sha256(stableStringify(candidateLedger));
  const productionLedgerHash = sha256(stableStringify(productionLedger));
  const actors = production.summary?.actors ?? {};
  const rawTotal = sumActorField(actors, "raw_damage");
  const rdpsTotal = sumActorField(actors, "rdps_damage");

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    protocol_pack_digest: PROTOCOL_PACK_DIGEST,
    effect_id: EFFECT_ID,
    candidate_scope: {
      recipient_class_ids: [11],
      primary_percent_raw_deltas: [750],
      provider_actor_id: "5",
      provider_entity_uuid: "5424024453760",
      recipient_actor_id: "7",
      recipient_entity_uuid: "216009015936",
      lifecycle_instance_ids: ["260"],
      damage_scripts: ["Attack"],
      other_recipient_classes_promoted: false,
      other_tiers_promoted: false,
      haste_or_action_opportunity_promoted: false,
      rejected_or_unresolved_rows_promoted: false,
    },
    attribution_contract: {
      authoritative_input: "observed integer damage",
      exact_fraction:
        "observed_damage * provider_removed_attack_coefficient_stage_body / active_attack_coefficient_stage_body",
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
        "unproven-downstream-cancellation-retained-for-revocation-audit-only",
      candidate_projection_value_must_not_be_displayed_as_rdps: true,
    },
    inputs: Object.fromEntries(
      Object.entries(inputPaths).map(([key, value]) => [key, descriptor(value)]),
    ),
    evidence: {
      exact_current_pack_lifecycle_replay_proven:
        equivalence.authority?.exact_current_pack_lifecycle_replay_proven,
      exact_current_pack_candidate_ledger_equivalence_proven:
        equivalence.authority?.exact_current_pack_candidate_ledger_equivalence_proven,
      exact_ordered_old_and_current_candidate_equivalence:
        equivalence.ledger_equivalence?.exact_ordered_equivalence,
      complete_gap_bounded_lifecycle_count:
        equivalence.exact_scope?.complete_gap_bounded_lifecycle_count,
      effect_lifecycle_event_count: equivalence.exact_scope?.effect_lifecycle_event_count,
      recipient_window_damage_event_count:
        equivalence.exact_scope?.recipient_window_damage_event_count,
      candidate_and_production_ledger_match: candidateLedgerHash === productionLedgerHash,
      candidate_ledger_sha256: candidateLedgerHash,
      production_ledger_sha256: productionLedgerHash,
      candidate_and_production_transfer_match:
        candidate.summary?.attributed_bonus_damage === production.summary?.attributed_bonus_damage &&
        candidate.emitted_contribution_events_by_effect?.[String(EFFECT_ID)] ===
          production.emitted_contribution_events_by_effect?.[String(EFFECT_ID)],
      production_attribution_mode: productionReplay.attribution_mode,
      production_runtime_target_match: production.runtime_target_match,
      production_candidate_audit_target_match: production.candidate_audit_target_match,
      production_emitted_rows:
        production.emitted_contribution_events_by_effect?.[String(EFFECT_ID)],
      replay_conserved: production.conserved,
      rational_projection_overflow_count:
        production.summary?.rational_projection_overflow_count,
      runtime_schema_version: runtime.schema_version,
      runtime_class_11_tier_0_lifecycle_authority:
        runtime.mechanical_power?.class_11_tier_0_current_pack_lifecycle_authority,
      runtime_class_11_tier_0_exact_rational_authority:
        runtime.mechanical_power?.class_11_tier_0_exact_rational_attribution_authority,
      runtime_server_integer_counterfactual_authority:
        runtime.mechanical_power?.server_integer_counterfactual_authority,
      runtime_unresolved_overlap_fails_closed:
        runtime.mechanical_power?.unresolved_overlap_fails_closed,
      runtime_projection_policy: runtime.mechanical_power?.rational_integer_projection,
      runtime_transfer_enabled: runtime.mechanical_power?.runtime_transfer_enabled,
      runtime_recipient_class_ids: runtime.mechanical_power?.runtime_recipient_class_ids,
      runtime_primary_percent_raw_deltas:
        runtime.mechanical_power?.runtime_primary_percent_raw_deltas,
      runtime_general_tier_formula_enabled:
        runtime.mechanical_power?.universal_tier_formula_enabled,
    },
    summary: {
      accepted_damage_rows: productionLedger.length,
      lifecycle_instances: ["260"],
      observed_damage_in_accepted_rows: productionLedger
        .reduce((total, row) => total + BigInt(row.observed_damage), 0n)
        .toString(),
      exact_contribution_numerator: exactNumerator.toString(),
      exact_contribution_denominator: exactDenominator.toString(),
      projected_integer_contribution: projected.toString(),
      replay_attributed_bonus_damage: String(production.summary?.attributed_bonus_damage),
      replay_raw_damage_total: rawTotal.toString(),
      replay_rdps_damage_total: rdpsTotal.toString(),
      provider_contribution_given: String(actors["5"]?.contribution_given),
      recipient_contribution_received: String(actors["7"]?.contribution_received),
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
      "Mechanical Power tiers other than the observed +750 transition remain unpromoted",
      "recipient classes other than 11 remain unpromoted",
      "rows without exact lifecycle, packet transition, damage stage, or overlap closure remain uncredited",
      "Mechanical Power haste and other action-opportunity effects remain uncredited",
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
  assert(stableStringify(report.candidate_scope?.primary_percent_raw_deltas) === "[750]",
    "Candidate scope must remain +750 only");
  assert(report.candidate_scope?.other_recipient_classes_promoted === false &&
    report.candidate_scope?.other_tiers_promoted === false &&
    report.candidate_scope?.haste_or_action_opportunity_promoted === false &&
    report.candidate_scope?.rejected_or_unresolved_rows_promoted === false,
  "Candidate scope broadened");
  assert(report.attribution_contract?.authoritative_input === "observed integer damage",
    "Observed integer damage is not authoritative");
  assert(report.attribution_contract?.integer_projection === PROJECTION_POLICY,
    "Wrong rational integer projection");
  assert(report.attribution_contract?.candidate_projection_status ===
    "unproven-downstream-cancellation-retained-for-revocation-audit-only" &&
    report.attribution_contract?.candidate_projection_value_must_not_be_displayed_as_rdps === true,
  "Unproven Mechanical Power projection regained display authority");
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
    "exact_current_pack_lifecycle_replay_proven",
    "exact_current_pack_candidate_ledger_equivalence_proven",
    "exact_ordered_old_and_current_candidate_equivalence",
    "candidate_and_production_ledger_match",
    "candidate_and_production_transfer_match",
    "production_runtime_target_match",
    "replay_conserved",
    "runtime_class_11_tier_0_lifecycle_authority",
    "runtime_unresolved_overlap_fails_closed",
  ]) assert(evidence[key] === true, `Required evidence is not true: ${key}`);
  assert(evidence.runtime_schema_version === 21, "Wrong runtime schema");
  assert(evidence.runtime_class_11_tier_0_exact_rational_authority === false &&
    evidence.runtime_transfer_enabled === false,
  "Revoked Mechanical Power proportional transfer regained runtime authority");
  assert(evidence.runtime_server_integer_counterfactual_authority === false,
    "Server integer authority must remain unclaimed");
  assert(evidence.runtime_general_tier_formula_enabled === false,
    "Universal tier authority must remain disabled");
  assert(evidence.runtime_projection_policy === PROJECTION_POLICY,
    "Runtime projection drifted");
  assert(stableStringify(evidence.runtime_recipient_class_ids) === "[]",
    "Runtime recipient scope must remain empty while the complete damage body is unresolved");
  assert(stableStringify(evidence.runtime_primary_percent_raw_deltas) === "[]",
    "Runtime tier scope must remain empty while the complete damage body is unresolved");
  assert(evidence.complete_gap_bounded_lifecycle_count === 5 &&
    evidence.effect_lifecycle_event_count === 10 &&
    evidence.recipient_window_damage_event_count === 4423,
  "Lifecycle proof scope drifted");
  assert(evidence.rational_projection_overflow_count === 0,
    "Exact rational projection overflowed");
  assert(evidence.production_attribution_mode === "production_promoted_rules" &&
    evidence.production_candidate_audit_target_match === false &&
    evidence.production_emitted_rows === 4261,
  "Replay did not use only promoted production rules");

  const summary = report.summary ?? {};
  assert(summary.accepted_damage_rows === 4261, "Unexpected accepted row count");
  assert(summary.projected_integer_contribution === "22100227",
    "Unexpected integer projection");
  assert(summary.replay_attributed_bonus_damage === "22100227",
    "Replay projection differs");
  assert(summary.replay_raw_damage_total === "2671673080" &&
    summary.replay_raw_damage_total === summary.replay_rdps_damage_total,
  "Ordinary and rDPS totals do not conserve");
  assert(summary.provider_contribution_given === "22100227" &&
    summary.recipient_contribution_received === "22100227",
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

function onlyReport(replay, label) {
  assert(Array.isArray(replay.reports) && replay.reports.length === 1,
    `${label} must contain exactly one report`);
  return replay.reports[0];
}

function descriptor(input) {
  const info = statSync(input);
  assert(info.size <= MAX_INPUT_BYTES, `Input exceeds ${MAX_INPUT_BYTES} bytes: ${input}`);
  return { path: path.resolve(input), bytes: info.size, sha256: sha256(readFileSync(input)) };
}

function readJson(input, label) {
  const info = statSync(input);
  assert(info.size <= MAX_INPUT_BYTES, `${label} exceeds ${MAX_INPUT_BYTES} bytes`);
  return JSON.parse(readFileSync(input, "utf8"));
}

function sumActorField(actors, field) {
  return Object.values(actors).reduce((total, actor) => total + BigInt(actor?.[field] ?? 0), 0n);
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

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function contentHash(report) {
  return sha256(stableStringify(withoutContentHash(report)));
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
  console.log("Mechanical Power exact-rational revocation receipt self-test passed.");
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(exitCode) {
  console.log(
    "Usage:\n" +
      "  node tools/bpsr-mechanical-power-exact-rational-promotion-proof.mjs build " +
      "--equivalence <json> --candidate-replay <json> --production-replay <json> " +
      "--runtime <json> --output <json>\n" +
      "  node tools/bpsr-mechanical-power-exact-rational-promotion-proof.mjs verify --input <json>\n" +
      "  node tools/bpsr-mechanical-power-exact-rational-promotion-proof.mjs self-test",
  );
  process.exit(exitCode);
}
