#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-packet-final-counterfactual-intervals.mjs";
const MODES = ["floor", "ceil", "nearest_half_up"];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(parsed) {
  const tracePath = path.resolve(required(parsed, "trace"));
  const outputPath = path.resolve(required(parsed, "output"));
  if (existsSync(outputPath)) throw new Error(`Refusing to overwrite existing output: ${outputPath}`);
  const trace = readJson(tracePath);
  const report = analyze(trace, tracePath);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(JSON.stringify({ output: outputPath, summary: report.summary }, null, 2));
}

function analyze(trace, tracePath) {
  assert(Number(trace.schema_version) === 6, "Expected Harmony trace schema 6");
  assert(Number(trace.effect_id) === 3_003_052, "Expected Harmony Grace effect 3003052");
  assert(String(trace.game_build) === "24687926", "Expected build 24687926");
  assert(Array.isArray(trace.traces) && trace.traces.length > 0, "Trace rows are required");

  const modes = Object.fromEntries(MODES.map((mode) => [mode, analyzeMode(trace.traces, mode)]));
  let exact = fraction(0n, 1n);
  for (const row of trace.traces) {
    exact = addFractions(exact, fraction(
      integer(row.arithmetic?.contribution_numerator, "contribution_numerator"),
      integer(row.arithmetic?.contribution_denominator, "contribution_denominator"),
    ));
  }
  const projected = roundHalfUp(exact.numerator, exact.denominator);
  const observedDamage = trace.traces.reduce(
    (total, row) => total + integer(row.observed_damage, "observed_damage"), 0n,
  );
  assert(projected > 0n && projected < observedDamage, "Projected contribution must conserve ordinary damage");

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: String(trace.game_build),
    effect_id: Number(trace.effect_id),
    source: descriptor(tracePath),
    policy: {
      packet_final_damage_is_authoritative: true,
      hidden_server_intermediate_is_not_required: true,
      server_integer_counterfactual_authority_claimed: false,
      candidate_integer_boundaries_are_solved_as_exact_intervals: true,
      exact_rational_accounting_is_distinct_from_hidden_server_integer_replay: true,
      ordinary_damage_is_unchanged: true,
      unresolved_damage_base_terms_are_not_invented: true,
      runtime_promotion_allowed: false,
    },
    summary: {
      damage_rows: trace.traces.length,
      observed_damage: observedDamage.toString(),
      exact_rational_stage_share: serialFraction(exact),
      canonical_sum_exact_then_half_up: projected.toString(),
      conserved_recipient_damage_after_projection: (observedDamage - projected).toString(),
      integer_boundary_modes: modes,
      maximum_per_hit_counterfactual_width: Math.max(
        ...Object.values(modes).map((mode) => mode.maximum_counterfactual_width),
      ),
    },
    adjudication: {
      removed_blocker: "missing server integer boundary",
      reason: "Each packet-final integer constrains the latent provider-removed final to a closed one- or two-integer set. The versioned accounting contract carries the exact rational stage share across all rows and performs one deterministic half-up projection per effect/provider/recipient, so no hidden server intermediate is requested or fabricated.",
      remaining_formula_obligation: "Prove that the active denominator contains every provider-sensitive base term in the correct order, including target mitigation and refined, elemental, or other defense-free Attack terms. The current Attack-times-coefficient body alone is not promoted as the complete damage base.",
      provider_rdps_credit_allowed: false,
      production_promotion_delta: 0,
    },
  };
}

function analyzeMode(rows, mode) {
  let exactRows = 0;
  let minimumContribution = 0n;
  let maximumContribution = 0n;
  let maximumWidth = 0n;
  const widthCounts = new Map();
  const examples = [];
  for (const row of rows) {
    const damage = integer(row.observed_damage, "observed_damage");
    const activeBody = integer(row.arithmetic?.active_stage_body, "active_stage_body");
    const withoutBody = integer(row.arithmetic?.without_provider_coefficient_term,
      "without_provider_coefficient_term") + integer(row.arithmetic?.fixed_parameter, "fixed_parameter");
    assert(activeBody > 0n && withoutBody >= 0n && withoutBody < activeBody,
      "Expected a positive adjacent provider-removed stage body");
    const interval = counterfactualInterval(damage, activeBody, withoutBody, mode);
    const width = interval.maximum - interval.minimum;
    maximumWidth = width > maximumWidth ? width : maximumWidth;
    widthCounts.set(width.toString(), (widthCounts.get(width.toString()) ?? 0) + 1);
    if (width === 0n) exactRows += 1;
    minimumContribution += damage - interval.maximum;
    maximumContribution += damage - interval.minimum;
    if (width > 0n && examples.length < 5) {
      examples.push({
        damage_sequence: row.damage_sequence,
        observed_damage: damage.toString(),
        active_stage_body: activeBody.toString(),
        provider_removed_stage_body: withoutBody.toString(),
        provider_removed_final_minimum: interval.minimum.toString(),
        provider_removed_final_maximum: interval.maximum.toString(),
      });
    }
  }
  return {
    exact_rows: exactRows,
    ambiguous_rows: rows.length - exactRows,
    maximum_counterfactual_width: Number(maximumWidth),
    width_counts: Object.fromEntries([...widthCounts].sort((a, b) => Number(a[0]) - Number(b[0]))),
    aggregate_provider_contribution_minimum: minimumContribution.toString(),
    aggregate_provider_contribution_maximum: maximumContribution.toString(),
    examples,
  };
}

function counterfactualInterval(damage, activeBody, withoutBody, mode) {
  if (mode === "floor") {
    return {
      minimum: floorDiv(withoutBody * damage, activeBody),
      maximum: ceilDiv(withoutBody * (damage + 1n), activeBody) - 1n,
    };
  }
  if (mode === "ceil") {
    return {
      minimum: floorDiv(withoutBody * (damage - 1n), activeBody) + 1n,
      maximum: ceilDiv(withoutBody * damage, activeBody),
    };
  }
  if (mode === "nearest_half_up") {
    return {
      minimum: floorDiv(withoutBody * (2n * damage - 1n) + activeBody, 2n * activeBody),
      maximum: ceilDiv(withoutBody * (2n * damage + 1n) + activeBody, 2n * activeBody) - 1n,
    };
  }
  throw new Error(`Unsupported mode: ${mode}`);
}

function verifyCommand(parsed) {
  const inputPath = path.resolve(required(parsed, "input"));
  const report = readJson(inputPath);
  verifyReport(report);
  console.log(`verified ${inputPath}`);
}

function verifyReport(report) {
  assert(Number(report.schema_version) === SCHEMA_VERSION, "Unexpected schema version");
  assert(report.generated_by === GENERATOR, "Unexpected generator");
  assert(String(report.game_build) === "24687926", "Unexpected build");
  assert(Number(report.effect_id) === 3_003_052, "Unexpected effect ID");
  assert(report.policy?.hidden_server_intermediate_is_not_required === true,
    "Hidden server intermediate must not be required");
  assert(report.policy?.server_integer_counterfactual_authority_claimed === false,
    "Server integer authority must remain unclaimed");
  assert(report.summary?.maximum_per_hit_counterfactual_width <= 1,
    "Packet-final inversion widened beyond one damage point");
  assert(report.adjudication?.removed_blocker === "missing server integer boundary",
    "The regressed blocker was not removed");
  assert(report.adjudication?.provider_rdps_credit_allowed === false,
    "Incomplete base composition cannot authorize provider credit");
  assert(report.content_sha256 === contentHash(report), "Content hash mismatch");
}

function selfTest() {
  const rows = [{
    damage_sequence: 1,
    observed_damage: "100",
    arithmetic: {
      active_stage_body: "10",
      without_provider_coefficient_term: "8",
      fixed_parameter: "0",
      contribution_numerator: "20",
      contribution_denominator: "1",
    },
  }];
  for (const mode of MODES) {
    const result = analyzeMode(rows, mode);
    assert(result.maximum_counterfactual_width <= 1, `${mode} test interval widened`);
  }
  assert(roundHalfUp(3n, 2n) === 2n, "Half-up projection changed");
  console.log(`${GENERATOR} self-test passed`);
}

function fraction(numerator, denominator) {
  assert(denominator > 0n, "Fraction denominator must be positive");
  const divisor = gcd(abs(numerator), denominator);
  return { numerator: numerator / divisor, denominator: denominator / divisor };
}
function addFractions(left, right) {
  return fraction(
    left.numerator * right.denominator + right.numerator * left.denominator,
    left.denominator * right.denominator,
  );
}
function serialFraction(value) {
  return { numerator: value.numerator.toString(), denominator: value.denominator.toString() };
}
function roundHalfUp(numerator, denominator) {
  return floorDiv(2n * numerator + denominator, 2n * denominator);
}
function floorDiv(numerator, denominator) {
  assert(numerator >= 0n && denominator > 0n, "Expected nonnegative division");
  return numerator / denominator;
}
function ceilDiv(numerator, denominator) {
  assert(numerator >= 0n && denominator > 0n, "Expected nonnegative division");
  return (numerator + denominator - 1n) / denominator;
}
function gcd(left, right) {
  while (right !== 0n) [left, right] = [right, left % right];
  return left || 1n;
}
function abs(value) { return value < 0n ? -value : value; }
function integer(value, label) {
  assert(value !== undefined && value !== null && /^-?\d+$/.test(String(value)), `${label} must be an integer`);
  return BigInt(value);
}
function descriptor(filePath) {
  const bytes = readFileSync(filePath);
  return { path: filePath.replaceAll("\\", "/"), bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex") };
}
function readJson(filePath) { return JSON.parse(readFileSync(filePath, "utf8")); }
function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}
function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    if (!key?.startsWith("--") || args[index + 1] === undefined) usage(1);
    parsed[key.slice(2)] = args[index + 1];
  }
  return parsed;
}
function required(parsed, key) {
  const value = parsed[key];
  if (!value) throw new Error(`Missing --${key}`);
  return value;
}
function assert(condition, message) { if (!condition) throw new Error(message); }
function usage(code) {
  console.error("Usage: bpsr-packet-final-counterfactual-intervals.mjs <build|verify|self-test> [options]");
  process.exit(code);
}
