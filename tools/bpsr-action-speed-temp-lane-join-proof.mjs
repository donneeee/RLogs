#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const EXPECTED_BUILD = "24687926";
const EXPECTED_EFFECT_ID = 31_602;
const RESPONSIVE_LANES = new Map([
  ["normal_attack_speed_attr_11720_plus_temporary_700", 700],
  ["guide_speed_attr_11730_plus_temporary_710", 710],
]);

function fail(message) {
  throw new Error(message);
}

function take(values, flag) {
  const index = values.indexOf(flag);
  if (index < 0 || index + 1 >= values.length) fail(`${flag} requires a value`);
  const value = values[index + 1];
  values.splice(index, 2);
  return value;
}

function parseArguments(argv) {
  const values = [...argv];
  const command = values.shift();
  if (command === "verify") {
    const input = path.resolve(take(values, "--input"));
    if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
    return { command, input };
  }
  if (command !== "generate") fail("expected generate or verify");
  const options = {
    command,
    build: take(values, "--build"),
    membership: path.resolve(take(values, "--membership")),
    actionSpeedProof: path.resolve(take(values, "--action-speed-proof")),
    tempAttrTable: path.resolve(take(values, "--temp-attr-table")),
    output: path.resolve(take(values, "--output")),
  };
  if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
  return options;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function receipt(file, bytes) {
  return { path: file, bytes: statSync(file).size, sha256: sha256(bytes) };
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return sha256(Buffer.from(JSON.stringify(copy)));
}

function integer(value) {
  const converted = Number(value);
  return Number.isSafeInteger(converted) ? converted : null;
}

function definitionsFrom(table) {
  return Object.values(table)
    .filter(
      (row) =>
        (Number(row?.AttrType) === 700 || Number(row?.AttrType) === 710) &&
        Number(row?.LogicType) === 1 &&
        row?.IsSyncClient === true,
    )
    .map((row) => ({
      config_id: Number(row.Id),
      effect_type: Number(row.AttrType),
      logic_type: Number(row.LogicType),
      skill_ids: (row.AttrParams ?? []).map(Number).filter(Number.isSafeInteger),
      lower_limit: integer(row.LowerLimit),
      upper_limit: integer(row.UpperLimit),
    }))
    .sort((left, right) => left.config_id - right.config_id);
}

function addDamage(total, membership) {
  return total + BigInt(membership?.ordinary_damage?.reported_amount_units ?? "0");
}

function validateReport(report) {
  const summary = report?.summary ?? {};
  const groups = report?.responsive_skill_groups ?? [];
  const groupMemberships = groups.reduce(
    (total, group) => total + Number(group.damage_action_memberships),
    0,
  );
  const groupDamage = groups.reduce(
    (total, group) => total + BigInt(group.reported_damage_units ?? "0"),
    0n,
  );
  if (
    Number(report?.schema_version) !== SCHEMA_VERSION ||
    report?.game_build !== EXPECTED_BUILD ||
    Number(report?.effect_id) !== EXPECTED_EFFECT_ID ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    report?.policy?.localized_names_are_runtime_keys !== false ||
    report?.native_lookup_semantics?.no_match_returns_false_with_zero_output !== true ||
    Number(summary.responsive_damage_action_memberships) !== groupMemberships ||
    BigInt(summary.responsive_reported_damage_units ?? "-1") !== groupDamage ||
    Number(summary.memberships_with_configured_temporary_speed_candidates) !==
      groups.reduce(
        (total, group) =>
          total + (group.matching_temp_attr_config_ids.length ? group.damage_action_memberships : 0),
        0,
      ) ||
    summary.runtime_temporary_speed_term_zero_allowed !== false ||
    summary.provider_rdps_credit_allowed !== false ||
    report.content_sha256 !== contentHash(report)
  ) {
    fail("action-speed temporary-lane join proof is inconsistent or unsafe");
  }
}

function generate(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports build ${EXPECTED_BUILD}`);
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const membershipBytes = readFileSync(options.membership);
  const actionSpeedBytes = readFileSync(options.actionSpeedProof);
  const tempAttrBytes = readFileSync(options.tempAttrTable);
  const membership = JSON.parse(membershipBytes);
  const actionSpeed = JSON.parse(actionSpeedBytes);
  const tempAttrTable = JSON.parse(tempAttrBytes);
  if (
    ![8, 9, 10].includes(Number(membership?.schema_version)) ||
    membership?.game_build !== EXPECTED_BUILD ||
    Number(membership?.effect_id) !== EXPECTED_EFFECT_ID ||
    Number(actionSpeed?.schema_version) !== 4 ||
    actionSpeed?.game_build !== EXPECTED_BUILD ||
    actionSpeed?.temporary_attribute_lookup?.semantic_operation !== "TryGetTempAttrByType" ||
    actionSpeed?.temporary_attribute_lookup?.no_match_returns_false_with_zero_output !== true
  ) {
    fail("inputs are not the reviewed exact-build membership and native speed frontier");
  }

  const definitions = definitionsFrom(tempAttrTable);
  const responsive = (membership.damage_action_memberships ?? []).filter((row) =>
    RESPONSIVE_LANES.has(row?.damage_route?.speed_lane),
  );
  const groups = new Map();
  for (const row of responsive) {
    const speedLane = row.damage_route.speed_lane;
    const effectType = RESPONSIVE_LANES.get(speedLane);
    const skillId = integer(row.damage_route.candidate_skill_id);
    if (skillId === null) fail("responsive membership lost its exact root skill ID");
    const key = `${effectType}:${skillId}`;
    const group = groups.get(key) ?? {
      temporary_effect_type: effectType,
      logic_type: 1,
      root_skill_id: skillId,
      speed_lane: speedLane,
      matching_temp_attr_config_ids: definitions
        .filter(
          (definition) =>
            definition.effect_type === effectType && definition.skill_ids.includes(skillId),
        )
        .map((definition) => definition.config_id),
      damage_action_memberships: 0,
      _damage: 0n,
      formula_authority: false,
    };
    group.damage_action_memberships += 1;
    group._damage = addDamage(group._damage, row);
    groups.set(key, group);
  }
  const projectedGroups = [...groups.values()]
    .map((group) => {
      const reportedDamage = group._damage.toString();
      delete group._damage;
      return { ...group, reported_damage_units: reportedDamage };
    })
    .sort(
      (left, right) =>
        left.temporary_effect_type - right.temporary_effect_type ||
        left.root_skill_id - right.root_skill_id,
    );
  const withCandidates = projectedGroups.filter(
    (group) => group.matching_temp_attr_config_ids.length > 0,
  );
  const responsiveDamage = responsive.reduce(addDamage, 0n);
  const candidateMemberships = withCandidates.reduce(
    (total, group) => total + group.damage_action_memberships,
    0,
  );
  const candidateDamage = withCandidates.reduce(
    (total, group) => total + BigInt(group.reported_damage_units),
    0n,
  );

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-action-speed-temp-lane-join-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    game_build: options.build,
    effect_id: EXPECTED_EFFECT_ID,
    proof_state: "exact-native-no-match-zero-and-current-build-static-candidate-absence-proven-runtime-table-promotion-open",
    inputs: {
      damage_action_membership_ledger: receipt(options.membership, membershipBytes),
      current_build_native_action_speed_proof: receipt(options.actionSpeedProof, actionSpeedBytes),
      current_build_temp_attr_table_candidate: receipt(options.tempAttrTable, tempAttrBytes),
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      localized_names_are_evidence_only: true,
      current_build_extracted_table_candidate_is_runtime_promotion_authority: false,
      missing_remote_cast_packets_required: false,
      missing_values_are_zero: false,
      ordinary_damage_totals_unchanged: true,
      provider_rdps_credit_allowed: false,
    },
    native_lookup_semantics: {
      operation: "TryGetTempAttrByType(effect_type, logic_type=1, skill_id, out signed_i32_sum)",
      exact_effect_types: [700, 710],
      exact_logic_type: 1,
      no_match_returns_false_with_zero_output: true,
      every_matching_entry_is_added_in_signed_i32_order: true,
    },
    current_build_speed_temp_definitions: definitions,
    responsive_skill_groups: projectedGroups,
    summary: {
      responsive_damage_action_memberships: responsive.length,
      responsive_reported_damage_units: responsiveDamage.toString(),
      distinct_responsive_root_skill_ids: new Set(
        projectedGroups.map((group) => group.root_skill_id),
      ).size,
      current_build_speed_temp_config_definitions: definitions.length,
      current_build_speed_temp_configured_skill_ids: new Set(
        definitions.flatMap((definition) => definition.skill_ids),
      ).size,
      memberships_with_configured_temporary_speed_candidates: candidateMemberships,
      reported_damage_units_with_configured_temporary_speed_candidates:
        candidateDamage.toString(),
      static_candidate_absence_for_every_responsive_membership: candidateMemberships === 0,
      native_no_match_zero_proven: true,
      runtime_temporary_speed_term_zero_allowed: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    blockers: [
      "the extracted TempAttrTable is a current-build static candidate and is not runtime promotion authority while the current-build protocol pack is missing",
      "construction of the live temporary-attribute dictionaries from the reviewed table has not been closed end to end",
      "damage-event time is not the unobserved remote action-start time",
      "ordinary action-speed attribute snapshots, provider-removed opportunity, integer rounding, and conservation remain open",
      "current-build protocol-pack identity and required replay gates remain missing",
    ],
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    `joined ${responsive.length} responsive memberships; ${candidateMemberships} have configured temporary-speed candidates; provider credit=false\nwrote ${options.output}\n`,
  );
}

const options = parseArguments(process.argv.slice(2));
if (options.command === "generate") generate(options);
else {
  const report = JSON.parse(readFileSync(options.input));
  validateReport(report);
  process.stdout.write(
    `verified ${report.summary.responsive_damage_action_memberships} responsive memberships; provider credit=false\n`,
  );
}
