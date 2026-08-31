#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-autoattack-family-transition-search.mjs";
const ATTACK_ATTRIBUTE_ID = 11330;
const ATTACK_FAMILY_IDS = new Set([11330, 11331, 11332]);

function fail(message) {
  throw new Error(message);
}

function parseArgs(argv) {
  const options = { inputs: [], exampleLimit: 64 };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (value === undefined) fail(`missing value for ${flag}`);
    index += 1;
    if (flag === "--input") options.inputs.push(value);
    else if (flag === "--output") options.output = value;
    else if (flag === "--example-limit") options.exampleLimit = Number(value);
    else fail(`unknown argument ${flag}`);
  }
  if (options.inputs.length === 0) fail("at least one --input is required");
  if (!options.output) fail("--output is required");
  if (!Number.isSafeInteger(options.exampleLimit) || options.exampleLimit < 0) {
    fail("--example-limit must be a nonnegative integer");
  }
  return options;
}

function sha256File(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, stable(value[key])]),
    );
  }
  return value;
}

function stableJson(value) {
  return JSON.stringify(stable(value));
}

function attributeValue(state, attributeId) {
  return state.find((row) => row.attribute_id === attributeId)?.value ?? null;
}

function withoutAttackFamily(state) {
  return state.filter((row) => !ATTACK_FAMILY_IDS.has(row.attribute_id));
}

function statusDifference(left, right) {
  const leftRows = new Map(left.map((row) => [stableJson(row), row]));
  const rightRows = new Map(right.map((row) => [stableJson(row), row]));
  return {
    removed: [...leftRows].filter(([key]) => !rightRows.has(key)).map(([, row]) => row),
    added: [...rightRows].filter(([key]) => !leftRows.has(key)).map(([, row]) => row),
  };
}

function statusesWithoutProviderIdentity(state) {
  return state.map((row) => ({
    effect_id: row.effect_id,
    stacks: row.stacks,
    level: row.level,
    origin_source_type_id: row.origin_source_type_id ?? null,
    origin_source_config_id: row.origin_source_config_id ?? null,
  }));
}

function attributeDifference(left, right) {
  const leftRows = new Map(left.map((row) => [row.attribute_id, row.value]));
  const rightRows = new Map(right.map((row) => [row.attribute_id, row.value]));
  const ids = new Set([...leftRows.keys(), ...rightRows.keys()]);
  return [...ids]
    .sort((a, b) => a - b)
    .filter((id) => leftRows.get(id) !== rightRows.get(id))
    .map((attributeId) => ({
      attribute_id: attributeId,
      low_value: leftRows.get(attributeId) ?? null,
      high_value: rightRows.get(attributeId) ?? null,
      delta:
        Number.isFinite(leftRows.get(attributeId)) && Number.isFinite(rightRows.get(attributeId))
          ? rightRows.get(attributeId) - leftRows.get(attributeId)
          : null,
    }));
}

function distance(left, right) {
  if (!left || !right) return null;
  const values = [left.x, left.y, left.z, right.x, right.y, right.z];
  if (!values.every(Number.isFinite)) return null;
  return Math.hypot(left.x - right.x, left.y - right.y, left.z - right.z);
}

function packetFlags(sample) {
  const packet = sample.packet ?? {};
  return {
    damage_source: sample.damage_source ?? null,
    damage_type: sample.damage_type ?? null,
    critical: sample.critical ?? null,
    lucky: sample.lucky ?? null,
    type_flags: packet.type_flags ?? null,
    normal_hit: packet.normal_hit ?? null,
    property: packet.property ?? null,
    rainbow: packet.rainbow ?? null,
    damage_mode: packet.damage_mode ?? null,
    passive_uuid: packet.passive_uuid ?? sample.passive_uuid ?? null,
    owner_level: packet.owner_level ?? null,
    owner_stage: packet.owner_stage ?? null,
  };
}

function structuralKey(sample, sourceAttributes, targetAttributes, targetStatuses) {
  return stableJson({
    session_id: sample.session_id,
    scene_id: sample.scene_id ?? null,
    source_entity_uuid: sample.source_entity_uuid,
    target_entity_uuid: sample.target_entity_uuid,
    ability_id: sample.ability_id,
    hit_event_id: sample.hit_event_id,
    source_actor_identity: sample.source_actor_identity ?? null,
    direct_source_actor_identity: sample.direct_source_actor_identity ?? null,
    target_actor_identity: sample.target_actor_identity ?? null,
    packet_flags: packetFlags(sample),
    source_attributes_except_attack_family: withoutAttackFamily(sourceAttributes),
    target_attributes: targetAttributes,
    target_statuses: targetStatuses,
  });
}

function relaxedSourceStateKey(sample, targetAttributes, targetStatuses) {
  return stableJson({
    session_id: sample.session_id,
    scene_id: sample.scene_id ?? null,
    source_entity_uuid: sample.source_entity_uuid,
    target_entity_uuid: sample.target_entity_uuid,
    ability_id: sample.ability_id,
    hit_event_id: sample.hit_event_id,
    source_actor_identity: sample.source_actor_identity ?? null,
    direct_source_actor_identity: sample.direct_source_actor_identity ?? null,
    target_actor_identity: sample.target_actor_identity ?? null,
    packet_flags: packetFlags(sample),
    target_attributes: targetAttributes,
    target_statuses: targetStatuses,
  });
}

function crossSessionKey(sample, sourceAttributes, targetAttributes, targetStatuses) {
  return stableJson({
    source_entity_uuid: sample.source_entity_uuid,
    ability_id: sample.ability_id,
    hit_event_id: sample.hit_event_id,
    source_actor_identity: sample.source_actor_identity ?? null,
    direct_source_actor_identity: sample.direct_source_actor_identity ?? null,
    target_actor_identity: sample.target_actor_identity ?? null,
    packet_flags: packetFlags(sample),
    source_attributes_except_attack_family: withoutAttackFamily(sourceAttributes),
    target_attributes: targetAttributes,
    target_statuses_without_provider_identity: statusesWithoutProviderIdentity(targetStatuses),
  });
}

function compactSample(sample, sourceAttributes, sourceStatuses) {
  return {
    session_id: sample.session_id,
    sequence: sample.sequence,
    observed_micros: sample.observed_micros,
    wire_capture_sequence: sample.wire_capture_sequence,
    source_entity_uuid: sample.source_entity_uuid,
    direct_source_entity_uuid: sample.direct_source_entity_uuid,
    target_entity_uuid: sample.target_entity_uuid,
    ability_id: sample.ability_id,
    hit_event_id: sample.hit_event_id,
    amount: sample.amount,
    normal_value: sample.normal_value,
    attack: attributeValue(sourceAttributes, ATTACK_ATTRIBUTE_ID),
    attack_add: attributeValue(sourceAttributes, 11332),
    packet_flags: packetFlags(sample),
    packet_position: sample.packet?.position ?? null,
    source_position: sample.source_position_at_wire_message_start ?? null,
    direct_source_position: sample.direct_source_position_at_wire_message_start ?? null,
    target_position: sample.target_position_at_wire_message_start ?? null,
    source_attributes: sourceAttributes,
    source_statuses: sourceStatuses,
  };
}

function comparePair(left, right) {
  const low = left.attack <= right.attack ? left : right;
  const high = low === left ? right : left;
  const status = statusDifference(low.source_statuses, high.source_statuses);
  return {
    identity: {
      session_id: low.session_id,
      source_entity_uuid: low.source_entity_uuid,
      target_entity_uuid: low.target_entity_uuid,
      ability_id: low.ability_id,
      hit_event_id: low.hit_event_id,
    },
    low: {
      sequence: low.sequence,
      wire_capture_sequence: low.wire_capture_sequence,
      direct_source_entity_uuid: low.direct_source_entity_uuid,
      attack: low.attack,
      attack_add: low.attack_add,
      amount: low.amount,
      normal_value: low.normal_value,
    },
    high: {
      sequence: high.sequence,
      wire_capture_sequence: high.wire_capture_sequence,
      direct_source_entity_uuid: high.direct_source_entity_uuid,
      attack: high.attack,
      attack_add: high.attack_add,
      amount: high.amount,
      normal_value: high.normal_value,
    },
    deltas: {
      attack: high.attack - low.attack,
      attack_add:
        Number.isFinite(high.attack_add) && Number.isFinite(low.attack_add)
          ? high.attack_add - low.attack_add
          : null,
      amount: high.amount - low.amount,
      normal_value:
        Number.isFinite(high.normal_value) && Number.isFinite(low.normal_value)
          ? high.normal_value - low.normal_value
          : null,
    },
    source_status_difference: status,
    source_attribute_difference: attributeDifference(
      low.source_attributes,
      high.source_attributes,
    ),
    relaxed_context: {
      same_session: low.session_id === high.session_id,
      same_target_entity_uuid: low.target_entity_uuid === high.target_entity_uuid,
      direct_source_entity_uuid_equal:
        low.direct_source_entity_uuid === high.direct_source_entity_uuid,
      packet_position_distance: distance(low.packet_position, high.packet_position),
      source_position_distance: distance(low.source_position, high.source_position),
      direct_source_position_distance: distance(
        low.direct_source_position,
        high.direct_source_position,
      ),
      target_position_distance: distance(low.target_position, high.target_position),
    },
  };
}

function scan(options) {
  const groups = new Map();
  const relaxedSourceStateGroups = new Map();
  const crossSessionGroups = new Map();
  const inputs = [];
  let selectedSamples = 0;
  let missingAttackSamples = 0;
  let skillEffectTotalDamageSamples = 0;
  let actualAmountSamples = 0;
  let peakInputBytes = 0;
  let gameBuild = null;

  for (const input of options.inputs) {
    const stat = fs.statSync(input);
    peakInputBytes = Math.max(peakInputBytes, stat.size);
    const cohort = JSON.parse(fs.readFileSync(input, "utf8"));
    if (cohort.schema_version !== 46) fail(`${input}: expected schema 46`);
    if (cohort.generated_by !== "rlogs-bpsr-state-scaling-damage-proof") {
      fail(`${input}: unexpected generator ${cohort.generated_by}`);
    }
    gameBuild ??= cohort.game_build;
    if (cohort.game_build !== gameBuild) fail(`${input}: mixed game builds`);
    inputs.push({ path: input, bytes: stat.size, sha256: sha256File(input) });

    for (const sample of cohort.samples) {
      selectedSamples += 1;
      if (sample.packet?.skill_effect_total_damage != null) {
        skillEffectTotalDamageSamples += 1;
      }
      if (sample.actual_amount != null) actualAmountSamples += 1;
      const sourceAttributes = cohort.attribute_states[sample.source_attribute_state_id];
      const targetAttributes = cohort.attribute_states[sample.target_attribute_state_id];
      const sourceStatuses = cohort.status_states[sample.source_status_state_id];
      const targetStatuses = cohort.status_states[sample.target_status_state_id];
      if (!sourceAttributes || !targetAttributes || !sourceStatuses || !targetStatuses) {
        fail(`${input}: sample ${sample.sequence} has an invalid state reference`);
      }
      const attack = attributeValue(sourceAttributes, ATTACK_ATTRIBUTE_ID);
      if (!Number.isFinite(attack)) {
        missingAttackSamples += 1;
        continue;
      }
      const compact = compactSample(sample, sourceAttributes, sourceStatuses);
      const key = structuralKey(sample, sourceAttributes, targetAttributes, targetStatuses);
      const rows = groups.get(key) ?? [];
      rows.push(compact);
      groups.set(key, rows);
      const relaxedKey = relaxedSourceStateKey(sample, targetAttributes, targetStatuses);
      const relaxedRows = relaxedSourceStateGroups.get(relaxedKey) ?? [];
      relaxedRows.push(compact);
      relaxedSourceStateGroups.set(relaxedKey, relaxedRows);
      const acrossRunsKey = crossSessionKey(
        sample,
        sourceAttributes,
        targetAttributes,
        targetStatuses,
      );
      const acrossRunsRows = crossSessionGroups.get(acrossRunsKey) ?? [];
      acrossRunsRows.push(compact);
      crossSessionGroups.set(acrossRunsKey, acrossRunsRows);
    }
  }

  let multiSampleGroups = 0;
  let multiAttackGroups = 0;
  let candidatePairs = 0;
  let statusTransitionPairs = 0;
  let exactDirectSourcePairs = 0;
  const attackDeltaCounts = new Map();
  const examples = [];

  for (const rows of groups.values()) {
    if (rows.length < 2) continue;
    multiSampleGroups += 1;
    const byAttack = new Map();
    for (const row of rows) {
      const values = byAttack.get(row.attack) ?? [];
      values.push(row);
      byAttack.set(row.attack, values);
    }
    const attacks = [...byAttack.keys()].sort((left, right) => left - right);
    if (attacks.length < 2) continue;
    multiAttackGroups += 1;
    for (let lowIndex = 0; lowIndex < attacks.length - 1; lowIndex += 1) {
      for (let highIndex = lowIndex + 1; highIndex < attacks.length; highIndex += 1) {
        for (const low of byAttack.get(attacks[lowIndex])) {
          for (const high of byAttack.get(attacks[highIndex])) {
            candidatePairs += 1;
            const pair = comparePair(low, high);
            const statusChanged =
              pair.source_status_difference.removed.length > 0 ||
              pair.source_status_difference.added.length > 0;
            if (statusChanged) statusTransitionPairs += 1;
            if (pair.relaxed_context.direct_source_entity_uuid_equal) {
              exactDirectSourcePairs += 1;
            }
            const delta = String(pair.deltas.attack);
            attackDeltaCounts.set(delta, (attackDeltaCounts.get(delta) ?? 0) + 1);
            if (examples.length < options.exampleLimit) examples.push(pair);
          }
        }
      }
    }
  }


  let relaxedSourceStateMultiSampleGroups = 0;
  let relaxedSourceStateMultiAttackGroups = 0;
  let relaxedSourceStatePairs = 0;
  let relaxedSourceStateStatusTransitionPairs = 0;
  const relaxedSourceStateAttackDeltaCounts = new Map();
  const relaxedSourceStateExamples = [];
  for (const rows of relaxedSourceStateGroups.values()) {
    if (rows.length < 2) continue;
    relaxedSourceStateMultiSampleGroups += 1;
    const byAttack = new Map();
    for (const row of rows) {
      const values = byAttack.get(row.attack) ?? [];
      values.push(row);
      byAttack.set(row.attack, values);
    }
    const attacks = [...byAttack.keys()].sort((left, right) => left - right);
    if (attacks.length < 2) continue;
    relaxedSourceStateMultiAttackGroups += 1;
    for (let lowIndex = 0; lowIndex < attacks.length - 1; lowIndex += 1) {
      for (let highIndex = lowIndex + 1; highIndex < attacks.length; highIndex += 1) {
        for (const low of byAttack.get(attacks[lowIndex])) {
          for (const high of byAttack.get(attacks[highIndex])) {
            relaxedSourceStatePairs += 1;
            const pair = comparePair(low, high);
            if (
              pair.source_status_difference.removed.length > 0 ||
              pair.source_status_difference.added.length > 0
            ) {
              relaxedSourceStateStatusTransitionPairs += 1;
            }
            const delta = String(pair.deltas.attack);
            relaxedSourceStateAttackDeltaCounts.set(
              delta,
              (relaxedSourceStateAttackDeltaCounts.get(delta) ?? 0) + 1,
            );
            if (relaxedSourceStateExamples.length < options.exampleLimit) {
              relaxedSourceStateExamples.push(pair);
            }
          }
        }
      }
    }
  }


  let crossSessionMultiSampleGroups = 0;
  let crossSessionMultiAttackGroups = 0;
  let crossSessionPairs = 0;
  let crossSessionStatusTransitionPairs = 0;
  const crossSessionAttackDeltaCounts = new Map();
  const crossSessionExamples = [];
  for (const rows of crossSessionGroups.values()) {
    if (rows.length < 2) continue;
    crossSessionMultiSampleGroups += 1;
    const byAttack = new Map();
    for (const row of rows) {
      const values = byAttack.get(row.attack) ?? [];
      values.push(row);
      byAttack.set(row.attack, values);
    }
    const attacks = [...byAttack.keys()].sort((left, right) => left - right);
    if (attacks.length < 2) continue;
    crossSessionMultiAttackGroups += 1;
    for (let lowIndex = 0; lowIndex < attacks.length - 1; lowIndex += 1) {
      for (let highIndex = lowIndex + 1; highIndex < attacks.length; highIndex += 1) {
        for (const low of byAttack.get(attacks[lowIndex])) {
          for (const high of byAttack.get(attacks[highIndex])) {
            crossSessionPairs += 1;
            const pair = comparePair(low, high);
            if (
              pair.source_status_difference.removed.length > 0 ||
              pair.source_status_difference.added.length > 0
            ) {
              crossSessionStatusTransitionPairs += 1;
            }
            const delta = String(pair.deltas.attack);
            crossSessionAttackDeltaCounts.set(
              delta,
              (crossSessionAttackDeltaCounts.get(delta) ?? 0) + 1,
            );
            if (crossSessionExamples.length < options.exampleLimit) {
              crossSessionExamples.push(pair);
            }
          }
        }
      }
    }
  }

  const output = {
    schema_version: 4,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    inputs,
    policy: {
      runtime_formula_authority: false,
      provider_rdps_credit_allowed: false,
      remote_player_cast_packets_required: false,
      current_character_snapshot_substitution_allowed: false,
      unresolved_evidence_preserved: true,
      memory_model:
        "inputs are parsed and released one at a time; only compact comparison rows are retained",
      exact_key:
        "same session, scene, source, target, ability, hit, actor identities, packet flags, all target attributes/statuses, and every source attribute except 11330/11331/11332",
      relaxed_dimensions:
        "source status state, direct-source entity instance, and packet/source/direct-source/target geometry are retained in each pair and are not silently treated as equal",
      relaxed_source_state_key:
        "same exact key except all source attributes and source statuses may differ; every difference is emitted for adjudication",
      cross_session_key:
        "same stable source, ability, hit, actor/monster identities, packet flags, all non-Attack source attributes, target attributes, and target status magnitude/origin fields; session, target entity instance, status-provider entity identity, source statuses, and geometry remain emitted relaxed evidence",
    },
    summary: {
      input_files: inputs.length,
      input_bytes: inputs.reduce((sum, input) => sum + input.bytes, 0),
      largest_single_input_bytes: peakInputBytes,
      selected_samples: selectedSamples,
      missing_attack_samples: missingAttackSamples,
      packet_native_pre_mitigation_field_coverage: {
        skill_effect_total_damage_non_null_samples: skillEffectTotalDamageSamples,
        actual_amount_non_null_samples: actualAmountSamples,
      },
      structural_groups: groups.size,
      multi_sample_groups: multiSampleGroups,
      multi_attack_groups: multiAttackGroups,
      candidate_pairs: candidatePairs,
      source_status_transition_pairs: statusTransitionPairs,
      same_direct_source_entity_pairs: exactDirectSourcePairs,
      attack_delta_counts: Object.fromEntries(
        [...attackDeltaCounts].sort((left, right) => Number(left[0]) - Number(right[0])),
      ),
      retained_examples: examples.length,
      retained_examples_truncated: examples.length < candidatePairs,
      relaxed_source_state_groups: relaxedSourceStateGroups.size,
      relaxed_source_state_multi_sample_groups: relaxedSourceStateMultiSampleGroups,
      relaxed_source_state_multi_attack_groups: relaxedSourceStateMultiAttackGroups,
      relaxed_source_state_pairs: relaxedSourceStatePairs,
      relaxed_source_state_status_transition_pairs: relaxedSourceStateStatusTransitionPairs,
      relaxed_source_state_attack_delta_counts: Object.fromEntries(
        [...relaxedSourceStateAttackDeltaCounts].sort(
          (left, right) => Number(left[0]) - Number(right[0]),
        ),
      ),
      retained_relaxed_source_state_examples: relaxedSourceStateExamples.length,
      retained_relaxed_source_state_examples_truncated:
        relaxedSourceStateExamples.length < relaxedSourceStatePairs,
      cross_session_groups: crossSessionGroups.size,
      cross_session_multi_sample_groups: crossSessionMultiSampleGroups,
      cross_session_multi_attack_groups: crossSessionMultiAttackGroups,
      cross_session_pairs: crossSessionPairs,
      cross_session_status_transition_pairs: crossSessionStatusTransitionPairs,
      cross_session_attack_delta_counts: Object.fromEntries(
        [...crossSessionAttackDeltaCounts].sort(
          (left, right) => Number(left[0]) - Number(right[0]),
        ),
      ),
      retained_cross_session_examples: crossSessionExamples.length,
      retained_cross_session_examples_truncated:
        crossSessionExamples.length < crossSessionPairs,
    },
    candidate_pair_examples: examples,
    relaxed_source_state_pair_examples: relaxedSourceStateExamples,
    cross_session_pair_examples: crossSessionExamples,
    conclusion: {
      exact_autoattack_operator_proven: false,
      exact_integer_rounding_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      next_step:
        candidatePairs > 0
          ? "Join retained pairs to exact DamageAttr coefficients and adjudicate only pairs whose relaxed source status, direct-source identity, geometry, and downstream random input are independently proven equivalent."
          : "No same-session AutoAttack pair changes Attack while preserving the exact structural key; controlled acquisition or authoritative server code remains required.",
    },
  };
  output.content_sha256 = crypto
    .createHash("sha256")
    .update(stableJson(output))
    .digest("hex");
  return output;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (fs.existsSync(options.output)) fail(`output already exists: ${options.output}`);
  const output = scan(options);
  fs.mkdirSync(path.dirname(options.output), { recursive: true });
  fs.writeFileSync(options.output, `${JSON.stringify(output, null, 2)}\n`);
  console.log(JSON.stringify(output.summary));
  console.log(`wrote ${options.output}`);
}

main();
