import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-harmony-grace-integer-boundary-envelope.mjs";
const EFFECT_ID = 3_003_052;
const SCALE = 10_000n;

function fail(message) {
  throw new Error(message);
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) fail(`invalid option near ${key ?? "<end>"}`);
    options[key.slice(2)] = value;
  }
  return options;
}

function required(options, key) {
  const value = options[key];
  if (!value) fail(`missing --${key}`);
  return value;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function contentSha256(value) {
  return sha256(Buffer.from(JSON.stringify(stable(value)), "utf8"));
}

function source(file) {
  const absolute = path.resolve(file);
  const bytes = readFileSync(absolute);
  return {
    path: path.relative(process.cwd(), absolute).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: sha256(bytes),
    value: JSON.parse(bytes.toString("utf8")),
  };
}

function receipt(entry) {
  return { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 };
}

function gcd(left, right) {
  left = left < 0n ? -left : left;
  right = right < 0n ? -right : right;
  while (right !== 0n) [left, right] = [right, left % right];
  return left;
}

function addFraction(total, numerator, denominator) {
  assert.ok(numerator >= 0n && denominator > 0n);
  const nextNumerator = total.numerator * denominator + numerator * total.denominator;
  const nextDenominator = total.denominator * denominator;
  const divisor = gcd(nextNumerator, nextDenominator);
  return {
    numerator: nextNumerator / divisor,
    denominator: nextDenominator / divisor,
  };
}

function roundHalfUp(numerator, denominator) {
  return (numerator * 2n + denominator) / (denominator * 2n);
}

function decimal(numerator, denominator, digits) {
  const whole = numerator / denominator;
  const remainder = numerator % denominator;
  const scale = 10n ** BigInt(digits);
  const fraction = (remainder * scale / denominator).toString().padStart(digits, "0");
  return `${whole}.${fraction}`;
}

function integerStage(operator, attack, coefficient, fixed) {
  const product = attack * coefficient;
  if (operator === "floor") return product / SCALE + fixed;
  if (operator === "ceil") return (product + SCALE - 1n) / SCALE + fixed;
  if (operator === "nearest_half_up") return (product + SCALE / 2n) / SCALE + fixed;
  fail(`unsupported integer operator ${operator}`);
}

function candidateFraction(operator, trace) {
  const observed = BigInt(trace.observed_damage);
  const attack = BigInt(trace.attack_final);
  const attackWithout = BigInt(trace.attack_without_provider);
  const coefficient = BigInt(trace.coefficient_basis_points);
  const fixed = BigInt(trace.fixed_parameter);
  if (operator === "unrounded_rational") {
    const activeNumerator = attack * coefficient + fixed * SCALE;
    const inactiveNumerator = attackWithout * coefficient + fixed * SCALE;
    return {
      numerator: observed * (activeNumerator - inactiveNumerator),
      denominator: activeNumerator,
      activeStage: { numerator: activeNumerator, denominator: SCALE },
      marginal: { numerator: activeNumerator - inactiveNumerator, denominator: SCALE },
    };
  }
  const active = integerStage(operator, attack, coefficient, fixed);
  const inactive = integerStage(operator, attackWithout, coefficient, fixed);
  return {
    numerator: observed * (active - inactive),
    denominator: active,
    activeStage: active,
    marginal: active - inactive,
  };
}

function buildReport(options) {
  const gameBuild = required(options, "build");
  const closure = source(required(options, "lifecycle-closure"));
  const audit = source(required(options, "audit"));

  assert.equal(closure.value.schema_version, 1);
  assert.equal(closure.value.game_build, gameBuild);
  assert.equal(closure.value.effect_id, EFFECT_ID);
  assert.equal(closure.value.proof?.all_candidate_damage_identities_replayed, true);
  assert.equal(closure.value.proof?.exact_formula_trace_for_every_candidate_row, true);
  assert.equal(closure.value.proof?.ordinary_damage_conserved, true);
  assert.equal(closure.value.summary?.replay_runtime_target_match, false);
  assert.equal(closure.value.policy?.packet_state_counterfactual_is_server_integer_observation, false);

  assert.ok(Number(audit.value.schema_version) >= 27);
  assert.equal(audit.value.harmony_grace_candidate_audit_enabled, true);
  assert.equal(audit.value.reports?.length, 1);
  const replay = audit.value.reports[0];
  assert.equal(replay.client_build, gameBuild);
  assert.equal(replay.session_id, closure.value.identity.session_id);
  assert.equal(replay.protocol_pack_digest, closure.value.identity.protocol_pack_digest);
  assert.equal(replay.runtime_target_match, false);
  assert.equal(replay.candidate_audit_target_match, true);
  assert.equal(replay.conserved, true);
  const rows = replay.emitted_contribution_ledger;
  assert.equal(rows.length, closure.value.summary.candidate_damage_rows);
  assert.equal(Number(replay.emitted_contribution_events_by_effect?.[EFFECT_ID]), rows.length);

  const operators = ["floor", "ceil", "nearest_half_up", "unrounded_rational"];
  const aggregates = {};
  for (const operator of operators) {
    let total = { numerator: 0n, denominator: 1n };
    let rowsDifferentFromFloor = 0;
    let zeroMarginalRows = 0;
    for (const row of rows) {
      assert.equal(Number(row.effect_id), EFFECT_ID);
      const trace = row.formula_trace;
      assert.ok(trace, `sequence ${row.sequence} missing formula trace`);
      const floor = candidateFraction("floor", trace);
      assert.equal(String(floor.activeStage), String(trace.active_stage_body));
      assert.equal(String(floor.marginal), String(trace.coefficient_stage_marginal));
      assert.equal(
        floor.numerator * BigInt(trace.contribution_denominator),
        BigInt(trace.contribution_numerator) * floor.denominator,
      );
      const candidate = candidateFraction(operator, trace);
      if (candidate.numerator === 0n) zeroMarginalRows += 1;
      if (candidate.numerator * floor.denominator !== floor.numerator * candidate.denominator) {
        rowsDifferentFromFloor += 1;
      }
      total = addFraction(total, candidate.numerator, candidate.denominator);
    }
    aggregates[operator] = {
      numerator: total.numerator.toString(),
      denominator: total.denominator.toString(),
      decimal: decimal(total.numerator, total.denominator, 9),
      aggregate_round_half_up: roundHalfUp(total.numerator, total.denominator).toString(),
      rows_different_from_floor: rowsDifferentFromFloor,
      zero_marginal_rows: zeroMarginalRows,
    };
  }

  assert.equal(aggregates.floor.numerator, closure.value.summary.exact_contribution.numerator);
  assert.equal(aggregates.floor.denominator, closure.value.summary.exact_contribution.denominator);
  assert.equal(aggregates.floor.aggregate_round_half_up,
    closure.value.summary.integer_projected_contribution);
  const projected = operators.map((operator) => BigInt(aggregates[operator].aggregate_round_half_up));
  const minimum = projected.reduce((left, right) => left < right ? left : right);
  const maximum = projected.reduce((left, right) => left > right ? left : right);

  const report = {
    schema_version: 1,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    effect_id: EFFECT_ID,
    identity: {
      session_id: replay.session_id,
      protocol_pack_digest: replay.protocol_pack_digest,
      provider_actor_ids: closure.value.identity.provider_actor_ids,
      recipient_actor_ids: closure.value.identity.recipient_actor_ids,
      lifecycle_instance_ids: closure.value.identity.lifecycle_instance_ids,
    },
    policy: {
      exact_numeric_ids_build_and_pack_are_authoritative: true,
      current_character_snapshots_used: false,
      candidate_boundaries_are_formula_authority: false,
      unresolved_integer_boundary_is_preserved: true,
      ordinary_damage_totals_may_change: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
    sources: {
      current_pack_lifecycle_closure: receipt(closure),
      exact_candidate_replay_audit: receipt(audit),
    },
    observed_scope: {
      damage_rows: rows.length,
      numeric_abilities: closure.value.summary.numeric_ability_count,
      allegiance_neutral_targets: closure.value.summary.allegiance_neutral_target_count,
      observed_damage: closure.value.summary.observed_damage,
      ordinary_raw_damage: closure.value.summary.ordinary_raw_damage,
      ordinary_rdps_damage: closure.value.summary.ordinary_rdps_damage,
      ordinary_damage_conserved: true,
    },
    candidate_integer_boundaries: aggregates,
    projection_envelope: {
      minimum_aggregate_integer: minimum.toString(),
      maximum_aggregate_integer: maximum.toString(),
      aggregate_integer_spread: (maximum - minimum).toString(),
      all_candidates_same_aggregate_integer: minimum === maximum,
    },
    conclusion: {
      current_floor_projection_reproduced_exactly: true,
      candidate_boundaries_materially_diverge: minimum !== maximum,
      exact_server_integer_boundary_proven: false,
      controlled_transition_still_required: true,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
  };
  return { ...report, content_sha256: contentSha256(report) };
}

function generate(options) {
  const output = path.resolve(required(options, "output"));
  const report = buildReport(options);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const input = path.resolve(required(options, "input"));
  const report = JSON.parse(readFileSync(input, "utf8"));
  const rebuilt = buildReport({
    build: report.game_build,
    "lifecycle-closure": report.sources.current_pack_lifecycle_closure.path,
    audit: report.sources.exact_candidate_replay_audit.path,
  });
  assert.deepEqual(report, rebuilt);
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-harmony-grace-integer-boundary-envelope.mjs generate --build <id> --lifecycle-closure <json> --audit <json> --output <json>\n  node tools/bpsr-harmony-grace-integer-boundary-envelope.mjs verify --input <json>");
  process.exit(1);
}
