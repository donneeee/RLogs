#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 5;
const EXPECTED_BUILD = "24687926";
const EXPECTED_EFFECT_ID = 31_602;
const FIXED_POINT_SCALE = 10_000n;

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

function argumentsFrom(argv) {
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
    damageTimeState: path.resolve(take(values, "--damage-time-state")),
    recipientModes: path.resolve(take(values, "--recipient-modes")),
    temporaryLane: path.resolve(take(values, "--temporary-lane")),
    actionSpeed: path.resolve(take(values, "--action-speed-proof")),
    membershipLedger: path.resolve(take(values, "--membership-ledger")),
    skillStageJoin: path.resolve(take(values, "--skill-stage-join")),
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

function gcd(left, right) {
  let a = left < 0n ? -left : left;
  let b = right < 0n ? -right : right;
  while (b !== 0n) [a, b] = [b, a % b];
  return a;
}

function fraction(numerator, denominator) {
  if (denominator <= 0n) fail("capacity denominator must be positive");
  const divisor = gcd(numerator, denominator);
  return {
    numerator: (numerator / divisor).toString(),
    denominator: (denominator / divisor).toString(),
  };
}

function sumFractions(fractions) {
  let numerator = 0n;
  let denominator = 1n;
  for (const value of fractions) {
    const nextNumerator = BigInt(value.numerator);
    const nextDenominator = BigInt(value.denominator);
    numerator = numerator * nextDenominator + nextNumerator * denominator;
    denominator *= nextDenominator;
    const divisor = gcd(numerator, denominator);
    numerator /= divisor;
    denominator /= divisor;
  }
  return { numerator: numerator.toString(), denominator: denominator.toString() };
}

function integer(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number)) fail(`${label} must be a safe integer`);
  return number;
}

function matchingModes(recipientModes, row) {
  return (recipientModes.recipient_modes ?? []).filter(
    (mode) =>
      mode.proof_state === "proven_reversible_recipient_mode_coefficient" &&
      integer(mode.attribute_id, "recipient mode attribute") === row.required_attribute_id &&
      (mode.target_entity_uuids ?? []).map(String).includes(String(row.damage_actor_entity_uuid)),
  );
}

function validateInputs(state, modes, temporary, actionSpeed, build) {
  if (
    build !== EXPECTED_BUILD ||
    Number(state?.schema_version) !== 4 ||
    state?.generated_by !== "rlogs-bpsr-action-speed-damage-time-state-proof" ||
    state?.game_build !== build ||
    Number(state?.effect_id) !== EXPECTED_EFFECT_ID ||
    state?.policy?.damage_event_time_is_action_start_time !== false ||
    state?.policy?.provider_rdps_credit_allowed !== false ||
    Number(state?.summary?.exact_damage_time_value_after_observation_memberships) !==
      Number(state?.summary?.responsive_damage_action_memberships) ||
    Number(modes?.schema_version) !== 1 ||
    modes?.generated_by !== "tools/bpsr-party-haste-recipient-mode-proof.mjs" ||
    modes?.game_build !== build ||
    Number(modes?.effect_id) !== EXPECTED_EFFECT_ID ||
    modes?.policy?.provider_rdps_credit_allowed !== false ||
    modes?.summary?.exact_damage_opportunity_counterfactual_proven !== false ||
    Number(temporary?.schema_version) !== 1 ||
    temporary?.game_build !== build ||
    Number(temporary?.effect_id) !== EXPECTED_EFFECT_ID ||
    temporary?.summary?.static_candidate_absence_for_every_responsive_membership !== true ||
    temporary?.summary?.native_no_match_zero_proven !== true ||
    temporary?.summary?.runtime_temporary_speed_term_zero_allowed !== false ||
    Number(actionSpeed?.schema_version) !== 5 ||
    actionSpeed?.game_build !== build ||
    actionSpeed?.summary?.exact_non_singing_algebraic_speed_formulas_proven !== true ||
    actionSpeed?.summary?.exact_native_float32_operation_order_proven !== true ||
    actionSpeed?.summary?.singing_native_float32_operation_order_proven !== true ||
    actionSpeed?.summary?.singing_offline_numeric_equivalence_proven !== false ||
    actionSpeed?.summary?.exact_temporary_attribute_match_operation_and_no_match_zero_proven !==
      true
  ) {
    fail("inputs are not the exact fail-closed effect-31602 capacity frontier");
  }
}

function validateMembershipAncestry(state, ledger, skillStageJoin, build) {
  const ledgerCopy = structuredClone(ledger);
  const ledgerHash = ledgerCopy.content_sha256;
  delete ledgerCopy.content_sha256;
  if (
    Number(ledger?.schema_version) !== 10 ||
    ledger?.generated_by !== "tools/bpsr-party-haste-damage-skill-join-proof.mjs" ||
    ledger?.game_build !== build ||
    Number(ledger?.effect_id) !== EXPECTED_EFFECT_ID ||
    ledger?.policy?.remote_player_cast_packets_required !== false ||
    ledger?.policy?.remote_player_cast_packets_synthesized !== false ||
    ledger?.policy?.packet_owner_stage_is_stage_type !== false ||
    ledger?.policy?.packet_owner_stage_is_zero_based_stage_logic_list_index_after_exact_skill_key_join !==
      true ||
    ledger?.summary?.provider_rdps_credit_allowed !== false ||
    Number(ledger?.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    ledgerHash !== sha256(Buffer.from(JSON.stringify(ledgerCopy))) ||
    Number(skillStageJoin?.schema_version) !== 2 ||
    skillStageJoin?.generated_by !== "tools/bpsr-action-speed-skill-table-join-proof.mjs" ||
    skillStageJoin?.game_build !== build ||
    Number(skillStageJoin?.effect_id) !== EXPECTED_EFFECT_ID ||
    skillStageJoin?.summary?.exact_packet_owner_stage_to_native_stage_type_proven_for_exact_skill_rows !==
      true ||
    skillStageJoin?.summary?.provider_rdps_credit_allowed !== false ||
    Number(skillStageJoin?.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    skillStageJoin?.content_sha256 !== contentHash(skillStageJoin)
  ) {
    fail("membership ancestry inputs are not the exact fail-closed current frontier");
  }
  const joinIndex = new Map();
  for (const row of skillStageJoin.rows ?? []) {
    const key = `${row.action_id}:${row.dictionary_kind}:${row.dictionary_key}:${row.packet_owner_stage}`;
    const previous = joinIndex.get(key);
    if (
      previous &&
      (previous.exact_packet_owner_stage_to_native_stage_type_proven !==
        row.exact_packet_owner_stage_to_native_stage_type_proven ||
        previous.native_stage_type !== row.native_stage_type ||
        previous.speed_lane !== row.speed_lane ||
        previous.skill_table_atk_speed_switch !== row.skill_table_atk_speed_switch)
    ) {
      fail(`conflicting duplicate skill-stage join key ${key}`);
    }
    if (!previous) joinIndex.set(key, row);
  }
  const ledgerIndex = new Map();
  for (const row of ledger.damage_action_memberships ?? []) {
    const key = `${row.session_id}:${row.sequence}`;
    if (ledgerIndex.has(key)) fail(`duplicate membership ledger key ${key}`);
    ledgerIndex.set(key, row);
  }
  let exactSkillMemberships = 0;
  let exactSkillDamage = 0n;
  let exactBulletMemberships = 0;
  let exactBulletDamage = 0n;
  for (const row of state.rows ?? []) {
    const key = `${row.session_id}:${row.sequence}`;
    const membership = ledgerIndex.get(key);
    const route = membership?.damage_route;
    const damage = BigInt(row.reported_damage_units);
    if (
      !membership ||
      String(route?.candidate_skill_id) !== String(row.root_skill_id) ||
      String(membership.effect_provider_actor_id) !== String(row.effect_provider_actor_id) ||
      String(membership.effect_provider_entity_uuid) !== String(row.effect_provider_entity_uuid) ||
      String(membership.effect_endpoint_actor_id) !== String(row.effect_endpoint_actor_id) ||
      String(membership.effect_endpoint_entity_uuid) !== String(row.effect_endpoint_entity_uuid) ||
      String(membership.damage_actor_id) !== String(row.damage_actor_id) ||
      String(membership.damage_actor_entity_uuid) !== String(row.damage_actor_entity_uuid) ||
      BigInt(membership.ordinary_damage?.reported_amount_units ?? "-1") !== damage ||
      route?.speed_lane !== row.speed_lane
    ) {
      fail(`damage-time row ${key} does not exactly match its membership ancestry`);
    }
    if (route.stage_logic_resolution === "exact-current-build-skill-effect-stage-index-to-stage-type") {
      const joinKey = `${membership.action_id}:${route.packet_owner_stage_dictionary_kind}:${route.selected_skill_logic_key}:${route.owner_stage}`;
      const joined = joinIndex.get(joinKey);
      if (
        joined?.exact_packet_owner_stage_to_native_stage_type_proven !== true ||
        Number(joined?.native_stage_type) !== Number(route.stage_type) ||
        joined?.speed_lane !== route.speed_lane
      ) {
        fail(`skill membership ${key} does not match the exact stage-type join`);
      }
      exactSkillMemberships += 1;
      exactSkillDamage += damage;
    } else if (
      route.stage_logic_resolution ===
        "exact-current-build-bullet-skill-effect-skill-id-uniform-stage-type" &&
      route.packet_owner_stage_dictionary_kind === "bullet" &&
      Number(route.stage_type) === 0 &&
      route.stage_family === "normal" &&
      route.speed_lane === "normal_attack_speed_attr_11720_plus_temporary_700"
    ) {
      exactBulletMemberships += 1;
      exactBulletDamage += damage;
    } else {
      fail(`responsive membership ${key} has no exact speed stage route`);
    }
  }
  return {
    exact_skill_stage_route_memberships: exactSkillMemberships,
    exact_skill_stage_route_reported_damage_units: exactSkillDamage.toString(),
    exact_bullet_uniform_initiating_skill_stage_route_memberships: exactBulletMemberships,
    exact_bullet_uniform_initiating_skill_stage_route_reported_damage_units:
      exactBulletDamage.toString(),
    exact_speed_stage_route_memberships: exactSkillMemberships + exactBulletMemberships,
    exact_speed_stage_route_reported_damage_units: (exactSkillDamage + exactBulletDamage).toString(),
  };
}

function buildAnalysis(state, modes) {
  const groups = new Map();
  const selfProvider = new Map();
  const unresolved = new Map();
  let coveredMemberships = 0;
  let coveredDamage = 0n;
  let unresolvedMemberships = 0;
  let unresolvedDamage = 0n;
  let selfProviderMemberships = 0;
  let selfProviderDamage = 0n;

  for (const row of state.rows ?? []) {
    if (
      row.damage_event_time_state_resolution !== "exact_damage_time_value_after_observation" ||
      row.attribute_value === null ||
      row.attribute_value === undefined ||
      row.action_start_time_state_proven !== false ||
      row.formula_authority !== false
    ) {
      fail("damage-time state row is not the exact fail-closed scalar frontier");
    }
    const damage = BigInt(row.reported_damage_units);
    if (
      String(row.effect_endpoint_actor_id) !== String(row.damage_actor_id) ||
      String(row.effect_endpoint_entity_uuid) !== String(row.damage_actor_entity_uuid)
    ) {
      fail("source-side effect endpoint stopped matching the damage actor");
    }
    const sameProviderActor = String(row.effect_provider_actor_id) === String(row.damage_actor_id);
    const sameProviderEntity =
      String(row.effect_provider_entity_uuid) === String(row.damage_actor_entity_uuid);
    if (sameProviderActor !== sameProviderEntity) {
      fail("provider self-identity is internally inconsistent");
    }
    if (sameProviderActor) {
      const key = `${row.damage_actor_entity_uuid}:${row.required_attribute_id}:${row.speed_lane}`;
      const group = selfProvider.get(key) ?? {
        effect_provider_actor_id: String(row.effect_provider_actor_id),
        effect_provider_entity_uuid: String(row.effect_provider_entity_uuid),
        damage_actor_id: String(row.damage_actor_id),
        damage_actor_entity_uuid: String(row.damage_actor_entity_uuid),
        required_attribute_id: integer(row.required_attribute_id, "required attribute"),
        speed_lane: row.speed_lane,
        exclusion_reason: "effect-provider-is-damage-actor-no-external-rdps-transfer",
        damage_action_memberships: 0,
        _damage: 0n,
        ordinary_damage_retained_by_damage_actor: true,
        provider_rdps_credit_allowed: false,
      };
      group.damage_action_memberships += 1;
      group._damage += damage;
      selfProvider.set(key, group);
      selfProviderMemberships += 1;
      selfProviderDamage += damage;
      continue;
    }
    const matches = matchingModes(modes, row);
    if (matches.length > 1) fail("damage-time row matches more than one recipient mode");
    if (matches.length === 0) {
      unresolvedMemberships += 1;
      unresolvedDamage += damage;
      const key = `${row.damage_actor_entity_uuid}:${row.required_attribute_id}`;
      const group = unresolved.get(key) ?? {
        damage_actor_entity_uuid: String(row.damage_actor_entity_uuid),
        required_attribute_id: integer(row.required_attribute_id, "required attribute"),
        speed_lane: row.speed_lane,
        reason: "no-proven-reversible-recipient-mode-for-exact-entity-and-attribute",
        damage_action_memberships: 0,
        _damage: 0n,
        provider_rdps_credit_allowed: false,
      };
      group.damage_action_memberships += 1;
      group._damage += damage;
      unresolved.set(key, group);
      continue;
    }

    const mode = matches[0];
    const coefficient = BigInt(integer(mode.proven_coefficient_units, "mode coefficient"));
    const observedAttribute = BigInt(integer(row.attribute_value, "observed attribute"));
    const observedSpeedNumerator = FIXED_POINT_SCALE + observedAttribute;
    const withoutProviderSpeedNumerator = observedSpeedNumerator - coefficient;
    if (coefficient <= 0n || withoutProviderSpeedNumerator <= 0n) {
      fail("recipient coefficient cannot produce a valid provider-removed speed");
    }
    const key = [
      row.damage_actor_entity_uuid,
      row.required_attribute_id,
      mode.recipient_class_id,
      mode.recipient_specialization_id,
      coefficient,
      observedAttribute,
    ].join(":");
    const group = groups.get(key) ?? {
      damage_actor_entity_uuid: String(row.damage_actor_entity_uuid),
      required_attribute_id: integer(row.required_attribute_id, "required attribute"),
      speed_lane: row.speed_lane,
      recipient_class_id: integer(mode.recipient_class_id, "recipient class"),
      recipient_specialization_id: integer(
        mode.recipient_specialization_id,
        "recipient specialization",
      ),
      provider_coefficient_units: coefficient.toString(),
      observed_attribute_units: observedAttribute.toString(),
      provider_removed_attribute_units: (observedAttribute - coefficient).toString(),
      observed_speed_ratio: fraction(observedSpeedNumerator, FIXED_POINT_SCALE),
      provider_removed_speed_ratio: fraction(withoutProviderSpeedNumerator, FIXED_POINT_SCALE),
      conditional_marginal_capacity_fraction: fraction(coefficient, observedSpeedNumerator),
      damage_action_memberships: 0,
      _damage: 0n,
      calculation_condition:
        "temporary term is zero and damage-time speed equals the unobserved action-opportunity speed",
      exact_action_opportunity_proven: false,
      integer_rounding_proven: false,
      formula_authority: false,
      provider_rdps_credit_allowed: false,
    };
    group.damage_action_memberships += 1;
    group._damage += damage;
    groups.set(key, group);
    coveredMemberships += 1;
    coveredDamage += damage;
  }

  const conditionalGroups = [...groups.values()]
    .map((group) => {
      const reportedDamage = group._damage;
      const coefficient = BigInt(group.provider_coefficient_units);
      const speedNumerator = FIXED_POINT_SCALE + BigInt(group.observed_attribute_units);
      delete group._damage;
      return {
        ...group,
        reported_damage_units: reportedDamage.toString(),
        conditional_provider_capacity_damage: fraction(
          reportedDamage * coefficient,
          speedNumerator,
        ),
      };
    })
    .sort(
      (left, right) =>
        left.damage_actor_entity_uuid.localeCompare(right.damage_actor_entity_uuid, "en") ||
        left.required_attribute_id - right.required_attribute_id ||
        Number(left.observed_attribute_units) - Number(right.observed_attribute_units),
    );
  const unresolvedGroups = [...unresolved.values()]
    .map((group) => {
      const damage = group._damage.toString();
      delete group._damage;
      return { ...group, reported_damage_units: damage };
    })
    .sort((left, right) =>
      left.damage_actor_entity_uuid.localeCompare(right.damage_actor_entity_uuid, "en"),
    );
  const selfProviderGroups = [...selfProvider.values()]
    .map((group) => {
      const damage = group._damage.toString();
      delete group._damage;
      return { ...group, reported_damage_units: damage };
    })
    .sort((left, right) =>
      left.damage_actor_entity_uuid.localeCompare(right.damage_actor_entity_uuid, "en"),
    );
  return {
    conditionalGroups,
    selfProviderGroups,
    unresolvedGroups,
    coveredMemberships,
    coveredDamage,
    unresolvedMemberships,
    unresolvedDamage,
    selfProviderMemberships,
    selfProviderDamage,
  };
}

function validateReport(report) {
  const summary = report?.summary ?? {};
  const conditionalGroups = report?.conditional_capacity_groups ?? [];
  const unresolvedGroups = report?.unresolved_recipient_mode_groups ?? [];
  const selfProviderGroups = report?.self_provider_exclusion_groups ?? [];
  const conditionalMemberships = conditionalGroups.reduce(
    (total, group) => total + Number(group.damage_action_memberships),
    0,
  );
  const conditionalDamage = conditionalGroups.reduce(
    (total, group) => total + BigInt(group.reported_damage_units),
    0n,
  );
  const unresolvedMemberships = unresolvedGroups.reduce(
    (total, group) => total + Number(group.damage_action_memberships),
    0,
  );
  const unresolvedDamage = unresolvedGroups.reduce(
    (total, group) => total + BigInt(group.reported_damage_units),
    0n,
  );
  const selfProviderMemberships = selfProviderGroups.reduce(
    (total, group) => total + Number(group.damage_action_memberships),
    0,
  );
  const selfProviderDamage = selfProviderGroups.reduce(
    (total, group) => total + BigInt(group.reported_damage_units),
    0n,
  );
  const aggregateConditionalCapacity = sumFractions(
    conditionalGroups.map((group) => group.conditional_provider_capacity_damage),
  );
  for (const group of conditionalGroups) {
    const coefficient = BigInt(group.provider_coefficient_units);
    const observed = BigInt(group.observed_attribute_units);
    const damage = BigInt(group.reported_damage_units);
    const expectedFraction = fraction(coefficient, FIXED_POINT_SCALE + observed);
    const expectedDamage = fraction(damage * coefficient, FIXED_POINT_SCALE + observed);
    if (
      JSON.stringify(group.conditional_marginal_capacity_fraction) !==
        JSON.stringify(expectedFraction) ||
      JSON.stringify(group.conditional_provider_capacity_damage) !==
        JSON.stringify(expectedDamage) ||
      group.exact_action_opportunity_proven !== false ||
      group.integer_rounding_proven !== false ||
      group.formula_authority !== false ||
      group.provider_rdps_credit_allowed !== false
    ) {
      fail("conditional capacity group changed or gained unsafe authority");
    }
  }
  for (const group of selfProviderGroups) {
    if (
      String(group.effect_provider_actor_id) !== String(group.damage_actor_id) ||
      String(group.effect_provider_entity_uuid) !== String(group.damage_actor_entity_uuid) ||
      group.exclusion_reason !==
        "effect-provider-is-damage-actor-no-external-rdps-transfer" ||
      group.ordinary_damage_retained_by_damage_actor !== true ||
      group.provider_rdps_credit_allowed !== false
    ) {
      fail("self-provider exclusion group is inconsistent or unsafe");
    }
  }
  if (
    Number(report?.schema_version) !== SCHEMA_VERSION ||
    report?.game_build !== EXPECTED_BUILD ||
    Number(report?.effect_id) !== EXPECTED_EFFECT_ID ||
    Number(summary.responsive_damage_action_memberships) !==
      conditionalMemberships + selfProviderMemberships + unresolvedMemberships ||
    BigInt(summary.responsive_reported_damage_units ?? "-1") !==
      conditionalDamage + selfProviderDamage + unresolvedDamage ||
    Number(summary.conditional_capacity_memberships) !== conditionalMemberships ||
    BigInt(summary.conditional_capacity_reported_damage_units ?? "-1") !== conditionalDamage ||
    Number(summary.unresolved_recipient_mode_memberships) !== unresolvedMemberships ||
    BigInt(summary.unresolved_recipient_mode_reported_damage_units ?? "-1") !==
      unresolvedDamage ||
    Number(summary.proven_self_provider_exclusion_memberships) !== selfProviderMemberships ||
    BigInt(summary.proven_self_provider_exclusion_reported_damage_units ?? "-1") !==
      selfProviderDamage ||
    summary.self_provider_damage_stays_with_damage_actor !== true ||
    JSON.stringify(summary.conditional_capacity_damage_exact_rational_sum) !==
      JSON.stringify(aggregateConditionalCapacity) ||
    report?.formula?.rationalized_capacity_is_exact_native_float32_replay !== false ||
    report?.policy?.rationalized_conditional_capacity_is_formula_authority !== false ||
    summary.exact_native_speed_float32_operation_order_proven !== true ||
    summary.exact_membership_speed_stage_route_proven !== true ||
    Number(summary.exact_skill_stage_route_memberships) !== 3_627 ||
    BigInt(summary.exact_skill_stage_route_reported_damage_units ?? "-1") !== 507_845_670n ||
    Number(summary.exact_bullet_uniform_initiating_skill_stage_route_memberships) !== 86 ||
    BigInt(
      summary.exact_bullet_uniform_initiating_skill_stage_route_reported_damage_units ?? "-1",
    ) !== 48_284_322n ||
    Number(summary.exact_speed_stage_route_memberships) !==
      Number(summary.responsive_damage_action_memberships) ||
    BigInt(summary.exact_speed_stage_route_reported_damage_units ?? "-1") !==
      BigInt(summary.responsive_reported_damage_units ?? "-2") ||
    summary.rationalized_conditional_capacity_is_exact_native_float32_replay !== false ||
    summary.exact_action_opportunity_proven !== false ||
    summary.integer_rounding_proven !== false ||
    summary.ui_rdps_display_allowed !== false ||
    summary.provider_rdps_credit_allowed !== false ||
    Number(summary.observed_damage_reassigned_to_provider) !== 0 ||
    report.content_sha256 !== contentHash(report)
  ) {
    fail("provider-removed capacity proof is inconsistent or unsafe");
  }
}

function generate(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports build ${EXPECTED_BUILD}`);
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const stateBytes = readFileSync(options.damageTimeState);
  const modeBytes = readFileSync(options.recipientModes);
  const temporaryBytes = readFileSync(options.temporaryLane);
  const actionSpeedBytes = readFileSync(options.actionSpeed);
  const membershipBytes = readFileSync(options.membershipLedger);
  const skillStageBytes = readFileSync(options.skillStageJoin);
  const state = JSON.parse(stateBytes);
  const modes = JSON.parse(modeBytes);
  const temporary = JSON.parse(temporaryBytes);
  const actionSpeed = JSON.parse(actionSpeedBytes);
  const membershipLedger = JSON.parse(membershipBytes);
  const skillStageJoin = JSON.parse(skillStageBytes);
  validateInputs(state, modes, temporary, actionSpeed, options.build);
  const membershipAncestry = validateMembershipAncestry(
    state,
    membershipLedger,
    skillStageJoin,
    options.build,
  );
  const analysis = buildAnalysis(state, modes);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-action-speed-provider-removed-capacity-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    game_build: options.build,
    effect_id: EXPECTED_EFFECT_ID,
    proof_state:
      "exact-damage-time-and-recipient-coefficient-conditional-capacity-proven-action-opportunity-open",
    inputs: {
      damage_time_speed_state: receipt(options.damageTimeState, stateBytes),
      reversible_recipient_modes: receipt(options.recipientModes, modeBytes),
      temporary_speed_lane_join: receipt(options.temporaryLane, temporaryBytes),
      native_action_speed_formula: receipt(options.actionSpeed, actionSpeedBytes),
      source_side_damage_action_membership_ledger: receipt(
        options.membershipLedger,
        membershipBytes,
      ),
      exact_skill_table_and_stage_type_join: receipt(options.skillStageJoin, skillStageBytes),
    },
    relationship_model: {
      provider_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      selected_effect_endpoint_damage_role: "damage_actor",
    },
    formula: {
      exact_native_speed_float32_operation_order:
        "add_f32(1.0f, div_f32(i32_to_f32(attribute), 10000.0f)); for configured temporary lanes, then add_f32(previous, div_f32(i32_to_f32(matching_temporary_term_or_zero), 10000.0f))",
      observed_speed: "(10000 + observed_attribute + temporary_term) / 10000",
      provider_removed_speed:
        "(10000 + observed_attribute - provider_coefficient + temporary_term) / 10000",
      conditional_marginal_capacity_fraction:
        "provider_coefficient / (10000 + observed_attribute)",
      conditional_capacity_damage:
        "reported_damage * provider_coefficient / (10000 + observed_attribute)",
      fixed_point_scale: 10_000,
      temporary_term_assumed_by_conditional_calculation: 0,
      rationalized_capacity_is_exact_native_float32_replay: false,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      current_character_snapshot_backfill_allowed: false,
      damage_event_time_is_action_start_time: false,
      missing_remote_cast_packets_required: false,
      hypothetical_capacity_is_observed_damage: false,
      hypothetical_capacity_may_be_reassigned_to_provider: false,
      self_provider_effect_may_transfer_damage_to_an_external_provider: false,
      ordinary_damage_totals_unchanged: true,
      rationalized_conditional_capacity_is_formula_authority: false,
      provider_rdps_credit_allowed: false,
      ui_rdps_display_allowed: false,
    },
    conditional_capacity_groups: analysis.conditionalGroups,
    self_provider_exclusion_groups: analysis.selfProviderGroups,
    unresolved_recipient_mode_groups: analysis.unresolvedGroups,
    summary: {
      responsive_damage_action_memberships:
        analysis.coveredMemberships +
        analysis.selfProviderMemberships +
        analysis.unresolvedMemberships,
      responsive_reported_damage_units: (
        analysis.coveredDamage + analysis.selfProviderDamage + analysis.unresolvedDamage
      ).toString(),
      conditional_capacity_groups: analysis.conditionalGroups.length,
      conditional_capacity_memberships: analysis.coveredMemberships,
      conditional_capacity_reported_damage_units: analysis.coveredDamage.toString(),
      conditional_capacity_damage_exact_rational_sum: sumFractions(
        analysis.conditionalGroups.map((group) => group.conditional_provider_capacity_damage),
      ),
      proven_self_provider_exclusion_memberships: analysis.selfProviderMemberships,
      proven_self_provider_exclusion_reported_damage_units:
        analysis.selfProviderDamage.toString(),
      self_provider_damage_stays_with_damage_actor: true,
      unresolved_recipient_mode_memberships: analysis.unresolvedMemberships,
      unresolved_recipient_mode_reported_damage_units: analysis.unresolvedDamage.toString(),
      exact_damage_time_speed_state_proven: true,
      reversible_recipient_coefficient_join_proven_for_conditional_rows: true,
      exact_native_speed_float32_operation_order_proven: true,
      exact_membership_speed_stage_route_proven: true,
      ...membershipAncestry,
      rationalized_conditional_capacity_is_exact_native_float32_replay: false,
      runtime_temporary_speed_term_zero_allowed: false,
      exact_action_opportunity_proven: false,
      integer_rounding_proven: false,
      packet_conservation_proven: false,
      ui_rdps_display_allowed: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    blockers: [
      "remote action-start packets are unavailable by transport design; exact action opportunity must instead be reconstructed from proven native scheduling, damage ancestry, and clock correspondence",
      "the current-build static TempAttrTable candidate is not yet end-to-end live dictionary authority",
      "native speed operation order is proven, but offline float32 bit equivalence, window-level action opportunity, and integer damage rounding remain unproven",
      "current-build protocol-pack identity and required replay gates remain missing",
    ],
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    `calculated ${analysis.conditionalGroups.length} conditional capacity groups covering ${analysis.coveredMemberships} memberships; provider credit=false\nwrote ${options.output}\n`,
  );
}

const options = argumentsFrom(process.argv.slice(2));
if (options.command === "generate") generate(options);
else {
  const report = JSON.parse(readFileSync(options.input));
  validateReport(report);
  process.stdout.write(
    `verified ${report.summary.conditional_capacity_groups} conditional capacity groups; provider credit=false\n`,
  );
}
