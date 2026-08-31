#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const EXPECTED_BUILD = "24687926";
const EXPECTED_EFFECT_ID = 31_602;
const RESPONSIVE_LANES = new Set([
  "normal_attack_speed_attr_11720_plus_temporary_700",
  "guide_speed_attr_11730_plus_temporary_710",
]);
const UNAFFECTED_LANE = "unaffected_normal_stage_atk_speed_switch_false";
const AMOUNT_FIELDS = ["reported_amount_units", "hp_loss_units", "shield_loss_units", "actual_amount_units"];

function fail(message) { throw new Error(message); }
function take(values, flag) {
  const index = values.indexOf(flag);
  if (index < 0 || index + 1 >= values.length) fail(`${flag} requires a value`);
  const result = values[index + 1];
  values.splice(index, 2);
  return result;
}
function args(argv) {
  const values = [...argv];
  const mode = values.shift();
  if (mode === "verify") {
    const input = path.resolve(take(values, "--input"));
    if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
    return { mode, input };
  }
  if (mode !== "analyze") fail("usage: analyze --damage-join <v7.json> --buff-ancestry <v2.json> --output <json> | verify --input <json>");
  const result = {
    mode,
    damageJoin: path.resolve(take(values, "--damage-join")),
    buffAncestry: path.resolve(take(values, "--buff-ancestry")),
    output: path.resolve(take(values, "--output")),
  };
  if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
  return result;
}
function sha256(bytes) { return createHash("sha256").update(bytes).digest("hex"); }
function receipt(file, bytes) { return { path: file, bytes: bytes.length, sha256: sha256(bytes) }; }
function parseJson(bytes) {
  const value = bytes.toString("utf8");
  return JSON.parse(value.charCodeAt(0) === 0xfeff ? value.slice(1) : value);
}
function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return sha256(Buffer.from(JSON.stringify(copy)));
}
function zeroTotals() {
  return Object.fromEntries(AMOUNT_FIELDS.map((field) => [field, 0n]));
}
function addTotals(target, source) {
  for (const field of AMOUNT_FIELDS) target[field] += BigInt(source?.[field] ?? "0");
}
function projectTotals(totals) {
  return Object.fromEntries(AMOUNT_FIELDS.map((field) => [field, totals[field].toString()]));
}
function eligibility(route) {
  if (route.speed_lane === UNAFFECTED_LANE) {
    return {
      eligibility: "proven-unaffected-by-effect-31602-speed-attribute",
      reason: "exact current-build normal stage with SkillTable.AtkSpeedSwitch=false",
    };
  }
  if (RESPONSIVE_LANES.has(route.speed_lane)) {
    return {
      eligibility: "potentially-speed-responsive-awaiting-counterfactual",
      reason: "exact current-build stage lane reads an attribute changed by effect 31602; action-time temporary term and provider-removed opportunity remain unresolved",
    };
  }
  if (route.resolution !== "exact-static-route-selected-by-observed-packet-damage-source") {
    return {
      eligibility: "unresolved-packet-damage-source-route",
      reason: "observed packet damage source did not select one exact current-build static route",
    };
  }
  if (route.damage_source === 2) {
    return {
      eligibility: "unresolved-buff-tick-speed-causality",
      reason: "BuffTable damage tick cadence versus initiating-skill opportunity is not proven",
    };
  }
  if (route.damage_source === 1) {
    return {
      eligibility: "unresolved-bullet-skill-ancestry",
      reason: "BulletTable route lacks one exact uniform initiating skill speed lane",
    };
  }
  return {
    eligibility: "unresolved-other-speed-causality",
    reason: "no reviewed effect-31602 speed lane applies",
  };
}

function summarize(rows, buffProof) {
  const groups = new Map();
  const total = zeroTotals();
  let memberships = 0;
  for (const row of rows) {
    const group = groups.get(row.eligibility) ?? { eligibility: row.eligibility, damage_memberships: 0, totals: zeroTotals() };
    group.damage_memberships += row.damage_memberships;
    addTotals(group.totals, row.damage_totals);
    groups.set(row.eligibility, group);
    memberships += row.damage_memberships;
    addTotals(total, row.damage_totals);
  }
  const partitions = [...groups.values()].sort((a, b) => a.eligibility.localeCompare(b.eligibility)).map((group) => ({
    eligibility: group.eligibility,
    damage_memberships: group.damage_memberships,
    damage_totals: projectTotals(group.totals),
  }));
  const unaffected = partitions.find((row) => row.eligibility === "proven-unaffected-by-effect-31602-speed-attribute");
  return {
    source_side_damage_action_memberships: memberships,
    ordinary_damage_totals: projectTotals(total),
    eligibility_partitions: partitions,
    proven_zero_speed_rdps_memberships: unaffected?.damage_memberships ?? 0,
    proven_zero_speed_rdps_reported_amount_units: unaffected?.damage_totals.reported_amount_units ?? "0",
    buff_memberships_with_exact_static_skill_lane_but_unproven_tick_causality:
      Number(buffProof?.summary?.exact_uniform_skill_speed_lane_memberships ?? 0),
    observed_damage_reassigned_to_provider: 0,
    provider_rdps_credit_allowed: false,
    runtime_promotion_allowed: false,
  };
}

function validate(report) {
  const summary = report?.summary ?? {};
  const partitions = summary.eligibility_partitions ?? [];
  const membershipSum = partitions.reduce((sum, row) => sum + Number(row.damage_memberships), 0);
  const amountSums = zeroTotals();
  for (const row of partitions) addTotals(amountSums, row.damage_totals);
  if (Number(report?.schema_version) !== SCHEMA_VERSION || report?.game_build !== EXPECTED_BUILD ||
      Number(report?.effect_id) !== EXPECTED_EFFECT_ID ||
      report?.policy?.potentially_responsive_is_provider_credit !== false ||
      report?.policy?.provider_rdps_credit_allowed !== false ||
      membershipSum !== Number(summary.source_side_damage_action_memberships) ||
      AMOUNT_FIELDS.some((field) => amountSums[field] !== BigInt(summary.ordinary_damage_totals?.[field] ?? "-1")) ||
      Number(summary.observed_damage_reassigned_to_provider) !== 0 ||
      summary.provider_rdps_credit_allowed !== false || summary.runtime_promotion_allowed !== false ||
      report.content_sha256 !== contentHash(report)) {
    fail("haste damage eligibility proof is inconsistent or unsafe");
  }
}

function analyze(options) {
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const joinBytes = readFileSync(options.damageJoin);
  const buffBytes = readFileSync(options.buffAncestry);
  const join = parseJson(joinBytes);
  const buffProof = parseJson(buffBytes);
  if (Number(join?.schema_version) < 7 || join?.game_build !== EXPECTED_BUILD ||
      Number(join?.effect_id) !== EXPECTED_EFFECT_ID || join?.policy?.provider_rdps_credit_allowed !== false ||
      Number(buffProof?.schema_version) < 2 || buffProof?.game_build !== EXPECTED_BUILD ||
      Number(buffProof?.effect_id) !== EXPECTED_EFFECT_ID || buffProof?.policy?.provider_rdps_credit_allowed !== false) {
    fail("inputs are not the exact current fail-closed effect-31602 frontier");
  }
  const rows = [];
  for (const action of join.action_candidates ?? []) {
    for (const route of action.packet_damage_route_observations ?? []) {
      const classification = eligibility(route);
      rows.push({
        action_id: action.action_id,
        damage_source: route.damage_source,
        damage_memberships: route.damage_memberships,
        speed_lane: route.speed_lane,
        stage_logic_resolution: route.stage_logic_resolution,
        packet_route_resolution: route.resolution,
        damage_totals: route.damage_totals,
        ...classification,
        provider_rdps_credit_allowed: false,
      });
    }
  }
  rows.sort((a, b) => b.damage_memberships - a.damage_memberships || String(a.action_id).localeCompare(String(b.action_id)));
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-haste-damage-eligibility-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    game_build: EXPECTED_BUILD,
    effect_id: EXPECTED_EFFECT_ID,
    inputs: {
      damage_skill_join: receipt(options.damageJoin, joinBytes),
      buff_static_ancestry: receipt(options.buffAncestry, buffBytes),
    },
    policy: {
      ordinary_damage_totals_unchanged: true,
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      proven_unaffected_lane_has_zero_speed_rdps: true,
      potentially_responsive_is_provider_credit: false,
      buff_static_skill_lane_is_tick_speed_causality: false,
      remote_player_cast_packets_required: false,
      provider_rdps_credit_allowed: false,
    },
    rows,
    summary: summarize(rows, buffProof),
    blockers: [
      "potentially responsive lanes still require event-time recipient attribute values and exact temporary terms 700 or 710",
      "provider-removed opportunity timing, operation order, integer rounding, and damage conservation remain unproven",
      "BuffTable periodic tick cadence and unresolved packet routes remain excluded",
      "current-build protocol-pack identity and required replay gates remain missing",
    ],
  };
  report.content_sha256 = contentHash(report);
  validate(report);
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`partitioned ${report.summary.source_side_damage_action_memberships} memberships; proven zero-speed damage=${report.summary.proven_zero_speed_rdps_reported_amount_units}; provider credit=false\n`);
}

const options = args(process.argv.slice(2));
if (options.mode === "verify") {
  const report = parseJson(readFileSync(options.input));
  validate(report);
  process.stdout.write(`verified ${report.summary.source_side_damage_action_memberships} memberships; provider credit=false\n`);
} else analyze(options);
