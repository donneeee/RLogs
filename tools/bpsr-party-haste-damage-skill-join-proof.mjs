#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream, existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import path from "node:path";

const SCHEMA_VERSION = 10;
const EXPECTED_BUILD = "24687926";
const EXPECTED_EFFECT_ID = 31_602;

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
  const mode = values.shift();
  if (mode === "verify") {
    const input = path.resolve(take(values, "--input"));
    if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
    return { mode, input };
  }
  if (mode !== "generate") fail(usage());
  const result = {
    mode,
    build: take(values, "--build"),
    timeline: path.resolve(take(values, "--timeline")),
    gapAudit: path.resolve(take(values, "--gap-audit")),
    skillTable: path.resolve(take(values, "--skill-table")),
    skillEffectTable: path.resolve(take(values, "--skill-effect-table")),
    buffTable: path.resolve(take(values, "--buff-table")),
    bulletTable: path.resolve(take(values, "--bullet-table")),
    damageAttrTable: path.resolve(take(values, "--damage-attr-table")),
    damageSourceRouteProof: path.resolve(take(values, "--damage-source-route-proof")),
    stageLogicCatalog: path.resolve(take(values, "--stage-logic-catalog")),
    output: path.resolve(take(values, "--output")),
  };
  if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
  return result;
}

function usage() {
  return "usage:\n  node tools/bpsr-party-haste-damage-skill-join-proof.mjs generate --build 24687926 --timeline <jsonl> --gap-audit <json> --skill-table <json> --skill-effect-table <json> --buff-table <json> --bullet-table <json> --damage-attr-table <json> --damage-source-route-proof <json> --stage-logic-catalog <json> --output <json>\n  node tools/bpsr-party-haste-damage-skill-join-proof.mjs verify --input <json>";
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return sha256(Buffer.from(JSON.stringify(copy)));
}

function receipt(file, bytes) {
  return { path: file, bytes: bytes.length, sha256: sha256(bytes) };
}

function parseJsonBytes(bytes) {
  const text = bytes.toString("utf8");
  return JSON.parse(text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
}

function optionalSafeInteger(value, field) {
  if (value === null || value === undefined) return null;
  const numeric = Number(value);
  if (!Number.isSafeInteger(numeric)) fail(`${field} is not an exact safe integer`);
  return numeric;
}

function optionalIntegerString(value, field) {
  const numeric = optionalSafeInteger(value, field);
  return numeric === null ? null : String(numeric);
}

function validateGapAudit(audit, build) {
  if (
    Number(audit?.schema_version) !== 3 ||
    audit?.generated_by !== "rlogs-bpsr-rlog-gap-window-audit" ||
    String(audit?.game_build) !== build ||
    Number(audit?.effect_id) !== EXPECTED_EFFECT_ID ||
    audit?.damage_relationship !== "source" ||
    audit?.policy?.damage_relationship_is_explicit !== true ||
    audit?.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    audit?.summary?.formula_authority !== false ||
    audit?.summary?.provider_rdps_credit_allowed !== false
  ) {
    fail("gap audit is not the exact source-side fail-closed effect-31602 frontier");
  }
}

function buildSkillIndexes(skillTable) {
  const direct = new Map();
  const byEffect = new Map();
  for (const row of Object.values(skillTable)) {
    if (!Number.isSafeInteger(Number(row?.Id))) continue;
    direct.set(Number(row.Id), row);
    for (const effectId of Array.isArray(row.EffectIDs) ? row.EffectIDs : []) {
      const rows = byEffect.get(Number(effectId)) ?? [];
      rows.push(row);
      byEffect.set(Number(effectId), rows);
    }
  }
  return { direct, byEffect };
}

function projectSkill(row, candidateRoute) {
  return {
    candidate_route: candidateRoute,
    skill_id: Number(row.Id),
    atk_speed_switch: row.AtkSpeedSwitch === true,
    skill_type: Number(row.SkillType),
    sync_stage_flag: row.SyncStageFlag === true,
    sing_or_guide_time: row.SingOrGuideTime ?? null,
    effect_ids: [...new Set((Array.isArray(row.EffectIDs) ? row.EffectIDs : [])
      .map(Number)
      .filter(Number.isSafeInteger))].sort((left, right) => left - right),
    localized_name_evidence: typeof row.Name === "string" ? row.Name : null,
  };
}

function buildStageLogicIndex(catalog, build) {
  if (
    ![1, 2, 3].includes(Number(catalog?.schema_version)) ||
    catalog?.generated_by !== "tools/bpsr-skill-logic-decoder" ||
    String(catalog?.build) !== build ||
    catalog?.authority?.exact_build_skill_logic_payload_decoded !== true ||
    catalog?.authority?.stage_logic_member_order_proven !== true ||
    catalog?.authority?.runtime_promotion_allowed !== false ||
    catalog?.authority?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(catalog?.skill_stages)
  ) {
    fail("stage-logic catalog is not the exact current-build fail-closed frontier");
  }
  const byDictionaryKey = new Map();
  for (const row of catalog.skill_stages) {
    const dictionaryKey = Number(row?.dictionary_key);
    const stageIndex = Number(row?.stage_index);
    const stageId = Number(row?.stage_id);
    const stageType = Number(row?.stage_type);
    if (
      row?.dictionary_kind !== "skill" ||
      !Number.isSafeInteger(dictionaryKey) ||
      !Number.isSafeInteger(stageIndex) ||
      !Number.isSafeInteger(stageId) ||
      !Number.isSafeInteger(stageType)
    ) continue;
    const stages = byDictionaryKey.get(dictionaryKey) ?? new Map();
    if (stages.has(stageIndex)) fail("stage-logic catalog has duplicate skill stage indexes");
    stages.set(stageIndex, { dictionary_key: dictionaryKey, stage_index: stageIndex, stage_id: stageId, stage_type: stageType });
    byDictionaryKey.set(dictionaryKey, stages);
  }
  return byDictionaryKey;
}

function stageFamily(stageType) {
  if (stageType === 0) return "normal";
  if (stageType === 1) return "singing";
  if (stageType === 2) return "charge";
  if (stageType === 3) return "aim";
  if (stageType === 4) return "aim_shooting";
  if (stageType === 8 || stageType === 9) return "guide";
  return "other";
}

function speedLane(stageType, atkSpeedSwitch) {
  if (stageType === 0) {
    return atkSpeedSwitch
      ? "normal_attack_speed_attr_11720_plus_temporary_700"
      : "unaffected_normal_stage_atk_speed_switch_false";
  }
  if (stageType === 1) return "singing_native_helper_numeric_boundary_open";
  if (stageType === 2) return "charge_speed_attr_11740";
  if (stageType === 8 || stageType === 9) {
    return "guide_speed_attr_11730_plus_temporary_710";
  }
  return "unaffected_stage_type";
}

function resolveStageLogic(action, route, stageLogicIndex, skillIndexes, skillEffectIndex) {
  const base = {
    candidate_skill_id: action.skill_table_candidates.length === 1
      ? action.skill_table_candidates[0].skill_id
      : null,
    candidate_skill_effect_ids: action.skill_table_candidates.length === 1
      ? action.skill_table_candidates[0].effect_ids
      : [],
    matching_skill_logic_keys: [],
    selected_skill_logic_key: null,
    owner_stage_index: route.owner_stage,
    stage_id: null,
    stage_type: null,
    stage_family: null,
    atk_speed_switch: action.skill_table_candidates.length === 1
      ? action.skill_table_candidates[0].atk_speed_switch
      : null,
    speed_lane: null,
    candidate_stage_indexes: [],
    stage_selection_basis: null,
    packet_owner_stage_dictionary_kind: null,
    stage_logic_resolution: "not-an-exact-skill-damage-route",
  };
  if (route.resolution !== "exact-static-route-selected-by-observed-packet-damage-source") {
    return base;
  }
  if (route.damage_source === 1 && route.selected_owner_tables.length === 1 &&
      route.selected_owner_tables[0] === "BulletTable") {
    const skillEffect = skillEffectIndex.get(action.action_id) ?? null;
    const rootSkillId = Number(skillEffect?.SkillId);
    const rootSkill = Number.isSafeInteger(rootSkillId) ? skillIndexes.direct.get(rootSkillId) : null;
    const rootEffectIds = rootSkill
      ? [...new Set((rootSkill.EffectIDs ?? []).map(Number).filter(Number.isSafeInteger))]
      : [];
    const stages = stageLogicIndex.get(action.action_id);
    const candidateStages = stages ? [...stages.values()].sort((a, b) => a.stage_index - b.stage_index) : [];
    const bulletBase = {
      ...base,
      candidate_skill_id: rootSkill ? rootSkillId : null,
      candidate_skill_effect_ids: rootEffectIds,
      matching_skill_logic_keys: candidateStages.length ? [action.action_id] : [],
      selected_skill_logic_key: candidateStages.length ? action.action_id : null,
      atk_speed_switch: rootSkill ? rootSkill.AtkSpeedSwitch === true : null,
      candidate_stage_indexes: candidateStages.map((row) => row.stage_index),
      stage_selection_basis: "all-current-build-skill-stages-must-share-one-speed-lane",
      packet_owner_stage_dictionary_kind: "bullet",
    };
    if (!skillEffect || !rootSkill || !rootEffectIds.includes(action.action_id)) {
      return { ...bulletBase, stage_logic_resolution: "unresolved-bullet-skill-effect-foreign-key" };
    }
    if (!candidateStages.length) {
      return { ...bulletBase, stage_logic_resolution: "unresolved-bullet-skill-logic-key" };
    }
    const stageTypes = [...new Set(candidateStages.map((row) => row.stage_type))];
    const speedLanes = [...new Set(stageTypes.map((type) => speedLane(type, bulletBase.atk_speed_switch)))];
    if (stageTypes.length !== 1 || speedLanes.length !== 1) {
      return { ...bulletBase, stage_logic_resolution: "ambiguous-bullet-skill-stage-speed-lanes" };
    }
    return {
      ...bulletBase,
      stage_type: stageTypes[0],
      stage_family: stageFamily(stageTypes[0]),
      speed_lane: speedLanes[0],
      stage_logic_resolution: "exact-current-build-bullet-skill-effect-skill-id-uniform-stage-type",
    };
  }
  if (route.damage_source !== 0) return base;
  if (action.skill_table_candidates.length !== 1) {
    return { ...base, stage_logic_resolution: "unresolved-skill-table-candidate-count" };
  }
  if (!Number.isSafeInteger(route.owner_stage) || route.owner_stage < 0) {
    return { ...base, stage_logic_resolution: "unresolved-missing-owner-stage-index" };
  }
  const matches = base.candidate_skill_effect_ids
    .map((effectId) => stageLogicIndex.get(effectId)?.get(route.owner_stage) ?? null)
    .filter(Boolean);
  const matchingKeys = [...new Set(matches.map((row) => row.dictionary_key))].sort((a, b) => a - b);
  base.matching_skill_logic_keys = matchingKeys;
  if (matchingKeys.length !== 1) {
    return {
      ...base,
      stage_logic_resolution: matchingKeys.length > 1
        ? "ambiguous-multiple-skill-effect-stage-index-matches"
        : "unresolved-no-skill-effect-stage-index-match",
    };
  }
  const stage = matches.find((row) => row.dictionary_key === matchingKeys[0]);
  return {
    ...base,
    selected_skill_logic_key: stage.dictionary_key,
    stage_id: stage.stage_id,
    stage_type: stage.stage_type,
    stage_family: stageFamily(stage.stage_type),
    speed_lane: speedLane(stage.stage_type, base.atk_speed_switch),
    candidate_stage_indexes: [stage.stage_index],
    stage_selection_basis: "packet-owner-stage-index-into-exact-skill-stage-logic-list",
    packet_owner_stage_dictionary_kind: "skill",
    stage_logic_resolution: "exact-current-build-skill-effect-stage-index-to-stage-type",
  };
}

function resolveCandidates(actionId, indexes) {
  const candidates = new Map();
  const direct = indexes.direct.get(actionId);
  if (direct) candidates.set(Number(direct.Id), projectSkill(direct, "direct_skill_id"));
  for (const row of indexes.byEffect.get(actionId) ?? []) {
    const skillId = Number(row.Id);
    const route = candidates.has(skillId) ? "direct_skill_id_and_effect_id" : "effect_id";
    candidates.set(skillId, projectSkill(row, route));
  }
  return [...candidates.values()].sort((left, right) => left.skill_id - right.skill_id);
}

function buildOwnerTableIndexes(tables) {
  return Object.entries(tables).map(([table, value]) => ({
    table,
    ids: new Set(Object.keys(value).map(Number)),
  }));
}

function resolveOwnerTableCandidates(actionId, indexes) {
  if (actionId === null) return [];
  return indexes.filter((index) => index.ids.has(actionId)).map((index) => index.table);
}

function buildDamageRouteIndex(proof, build) {
  if (
    Number(proof?.schema_version) !== 9 ||
    String(proof?.game_build) !== build ||
    proof?.generated_by !== "rlogs-bpsr-damage-source-route-proof" ||
    proof?.policy?.packet_damage_source_required !== true ||
    proof?.policy?.runtime_formula_authority !== false ||
    !Array.isArray(proof?.keys)
  ) {
    fail("damage-source route proof is not the exact current-build candidate frontier");
  }
  return new Map(proof.keys.map((row) => [row.lookup_key, row]));
}

function damageRouteObservation(actionId, damage, routeIndex) {
  const hitEventId = damage?.hit_event_id;
  const damageSource = damage?.damage_source;
  const ownerStage = damage?.packet?.owner_stage;
  const ownerLevel = damage?.packet?.owner_level;
  const lookupKey =
    actionId !== null && Number.isSafeInteger(Number(hitEventId))
      ? `${actionId}:${Number(hitEventId)}`
      : null;
  const route = lookupKey === null ? null : routeIndex.get(lookupKey) ?? null;
  const selections =
    route && Number.isSafeInteger(Number(damageSource))
      ? (route.selection_by_damage_source ?? []).filter(
          (candidate) => Number(candidate.damage_source_id) === Number(damageSource),
        )
      : [];
  const selectedDamageAttrIds = [...new Set(selections.map((row) => Number(row.damage_attr_id)))];
  const ownerTables = [];
  for (const damageAttrId of selectedDamageAttrIds) {
    const candidate = (route?.candidates ?? []).find(
      (row) => Number(row.damage_attr_id) === damageAttrId,
    );
    for (const candidateRoute of candidate?.routes ?? []) {
      if (Number(candidateRoute.damage_source_id) === Number(damageSource)) {
        ownerTables.push(candidateRoute.owner_table);
      }
    }
  }
  return {
    lookup_key: lookupKey,
    hit_event_id: Number.isSafeInteger(Number(hitEventId)) ? Number(hitEventId) : null,
    damage_source: Number.isSafeInteger(Number(damageSource)) ? Number(damageSource) : null,
    owner_stage: Number.isSafeInteger(Number(ownerStage)) ? Number(ownerStage) : null,
    owner_level: Number.isSafeInteger(Number(ownerLevel)) ? Number(ownerLevel) : null,
    selected_damage_attr_ids: selectedDamageAttrIds,
    selected_owner_tables: [...new Set(ownerTables)].sort(),
    resolution:
      selectedDamageAttrIds.length === 1
        ? "exact-static-route-selected-by-observed-packet-damage-source"
        : selectedDamageAttrIds.length > 1
          ? "ambiguous-static-route-after-observed-packet-damage-source"
          : "unresolved-static-route",
  };
}

function activeWindows(audit) {
  const windows = [];
  for (const session of audit.sessions ?? []) {
    for (const window of session.complete_gap_bounded_windows ?? []) {
      if (Number(window.damage_events_while_active) <= 0) continue;
      if (window.effect_endpoint_damage_role !== "damage_actor") {
        fail("source-side gap window lost its damage_actor endpoint role");
      }
      windows.push({
        window_ordinal: windows.length + 1,
        session_id: session.session_id,
        target_actor_id: String(window.target_actor_id),
        target_entity_uuid: String(window.target_entity_uuid),
        source_actor_id: String(window.source_actor_id),
        source_entity_uuid: String(window.source_entity_uuid),
        status_instance_id: Number(window.instance_id),
        applied_sequence: Number(window.applied_envelope_sequence),
        terminal_sequence: Number(window.terminal_envelope_sequence),
        expected_memberships: Number(window.damage_events_while_active),
        observed_memberships: 0,
      });
    }
  }
  return windows;
}

function emptyDamageTotals() {
  return {
    reported_amount_units: 0n,
    hp_loss_units: 0n,
    shield_loss_units: 0n,
    actual_amount_units: 0n,
    events_with_reported_amount: 0,
    events_with_hp_loss: 0,
    events_with_shield_loss: 0,
    events_with_actual_amount: 0,
  };
}

function accumulateDamageTotals(totals, damage) {
  for (const [field, output, count] of [
    ["amount", "reported_amount_units", "events_with_reported_amount"],
    ["hp_loss", "hp_loss_units", "events_with_hp_loss"],
    ["shield_loss", "shield_loss_units", "events_with_shield_loss"],
    ["actual_amount", "actual_amount_units", "events_with_actual_amount"],
  ]) {
    const value = damage?.[field];
    if (value === null || value === undefined) continue;
    if (!Number.isSafeInteger(Number(value))) continue;
    totals[output] += BigInt(value);
    totals[count] += 1;
  }
}

function projectDamageTotals(totals) {
  return {
    reported_amount_units: totals.reported_amount_units.toString(),
    hp_loss_units: totals.hp_loss_units.toString(),
    shield_loss_units: totals.shield_loss_units.toString(),
    actual_amount_units: totals.actual_amount_units.toString(),
    events_with_reported_amount: totals.events_with_reported_amount,
    events_with_hp_loss: totals.events_with_hp_loss,
    events_with_shield_loss: totals.events_with_shield_loss,
    events_with_actual_amount: totals.events_with_actual_amount,
  };
}

async function analyzeTimeline(file, audit, skillIndexes, ownerTableIndexes, damageRouteIndex, stageLogicIndex, skillEffectIndex) {
  const windows = activeWindows(audit);
  const actionRows = new Map();
  const memberships = [];
  const hash = createHash("sha256");
  const stream = createReadStream(file);
  stream.on("data", (chunk) => hash.update(chunk));
  const lines = createInterface({ input: stream, crlfDelay: Infinity });
  let manifest = null;
  let observedMemberships = 0;

  for await (const line of lines) {
    if (!line.includes("\"row_type\"") || !line.trim()) continue;
    let row;
    try {
      row = JSON.parse(line);
    } catch {
      fail("support timeline contains malformed JSONL");
    }
    if (row.row_type === "manifest") {
      manifest ??= row;
      continue;
    }
    if (row.row_type !== "relationship" || row.event_kind !== "damage") continue;
    const damageActorId = String(row.damage_actor_id ?? "");
    const matching = windows.filter(
      (window) =>
        window.session_id === row.session_id &&
        window.target_actor_id === damageActorId &&
        Number(row.sequence) > window.applied_sequence &&
        Number(row.sequence) < window.terminal_sequence,
    );
    for (const window of matching) {
      window.observed_memberships += 1;
      observedMemberships += 1;
      const actionKey = row.action_id === null ? "null" : String(row.action_id);
      let action = actionRows.get(actionKey);
      if (!action) {
        const actionId = row.action_id === null ? null : Number(row.action_id);
        action = {
          action_id: actionId,
          damage_memberships: 0,
          skill_table_candidates:
            actionId === null ? [] : resolveCandidates(actionId, skillIndexes),
          owner_id_table_candidates: resolveOwnerTableCandidates(actionId, ownerTableIndexes),
          action_instance_observed: false,
          formula_authority: false,
          _route_observations: new Map(),
        };
        actionRows.set(actionKey, action);
      }
      action.damage_memberships += 1;
      const damage = row.canonical_event?.data ?? null;
      const route = damageRouteObservation(action.action_id, damage, damageRouteIndex);
      const enrichedRoute = {
        ...route,
        ...resolveStageLogic(action, route, stageLogicIndex, skillIndexes, skillEffectIndex),
      };
      const membershipDamageTotals = emptyDamageTotals();
      accumulateDamageTotals(membershipDamageTotals, damage);
      const packet = damage?.packet ?? {};
      const packetDamageGroup = {
        skill_effect_uuid: optionalIntegerString(
          packet.skill_effect_uuid,
          "damage.packet.skill_effect_uuid",
        ),
        total_damage_units: optionalIntegerString(
          packet.skill_effect_total_damage,
          "damage.packet.skill_effect_total_damage",
        ),
        group_index: optionalSafeInteger(
          packet.skill_effect_group_index,
          "damage.packet.skill_effect_group_index",
        ),
        component_index: optionalSafeInteger(
          packet.skill_effect_component_index,
          "damage.packet.skill_effect_component_index",
        ),
        component_count: optionalSafeInteger(
          packet.skill_effect_component_count,
          "damage.packet.skill_effect_component_count",
        ),
      };
      memberships.push({
        session_id: row.session_id,
        sequence: Number(row.sequence),
        observed_micros: Number(row.observed_micros),
        game_time_millis: optionalSafeInteger(row.game_time_millis, "relationship.game_time_millis"),
        capture_sequence: Number(row.capture_sequence),
        window_ordinal: window.window_ordinal,
        status_instance_id: window.status_instance_id,
        effect_provider_actor_id: window.source_actor_id,
        effect_provider_entity_uuid: window.source_entity_uuid,
        effect_endpoint_actor_id: window.target_actor_id,
        effect_endpoint_entity_uuid: window.target_entity_uuid,
        damage_actor_id: String(row.damage_actor_id),
        damage_actor_entity_uuid: String(row.damage_actor_entity_uuid),
        damage_target_actor_id: String(row.damage_target_actor_id),
        damage_target_entity_uuid: String(row.damage_target_entity_uuid),
        action_id: action.action_id,
        packet_damage_group: packetDamageGroup,
        packet_damage_group_is_remote_action_instance: false,
        damage_route: enrichedRoute,
        ordinary_damage: projectDamageTotals(membershipDamageTotals),
        exact_action_time_speed_state_proven: false,
        formula_authority: false,
      });
      const routeKey = JSON.stringify(enrichedRoute);
      const retained = action._route_observations.get(routeKey) ?? {
        ...enrichedRoute,
        damage_memberships: 0,
        formula_authority: false,
        _damage_totals: emptyDamageTotals(),
      };
      retained.damage_memberships += 1;
      accumulateDamageTotals(retained._damage_totals, damage);
      action._route_observations.set(routeKey, retained);
    }
  }

  if (!manifest || ![7, 8].includes(Number(manifest.schema_version))) {
    fail("support timeline manifest schema is unsupported");
  }
  const mismatchedWindows = windows.filter(
    (window) => window.observed_memberships !== window.expected_memberships,
  );
  if (mismatchedWindows.length) fail("sequence join did not conserve every source-side window membership");
  return {
    timeline: {
      path: file,
      bytes: statSync(file).size,
      sha256: hash.digest("hex"),
      schema_version: Number(manifest.schema_version),
    },
    windows,
    observedMemberships,
    memberships,
    actions: [...actionRows.values()]
      .map((action) => {
        const routeObservations = [...action._route_observations.values()].map((route) => {
          const damageTotals = projectDamageTotals(route._damage_totals);
          delete route._damage_totals;
          return { ...route, damage_totals: damageTotals };
        }).sort(
          (left, right) => right.damage_memberships - left.damage_memberships,
        );
        delete action._route_observations;
        return { ...action, packet_damage_route_observations: routeObservations };
      })
      .sort(
        (left, right) =>
          right.damage_memberships - left.damage_memberships ||
          String(left.action_id).localeCompare(String(right.action_id)),
      ),
  };
}

function summarize(analysis) {
  let unique = 0;
  let ambiguous = 0;
  let unmapped = 0;
  let uniqueOwnerTable = 0;
  let ambiguousOwnerTable = 0;
  let unmappedOwnerTable = 0;
  let exactPacketRoute = 0;
  let ambiguousPacketRoute = 0;
  let unresolvedPacketRoute = 0;
  let exactSkillRoute = 0;
  let exactSkillRouteWithOwnerStage = 0;
  let exactSkillRouteAtkSpeedSwitchTrue = 0;
  let exactSkillRouteAtkSpeedSwitchFalse = 0;
  let exactBulletRoute = 0;
  let stageFamilyEligible = 0;
  let exactStageFamily = 0;
  let ambiguousStageFamily = 0;
  let unresolvedStageFamily = 0;
  const stageTypeCounts = new Map();
  const stageFamilyCounts = new Map();
  const speedLaneCounts = new Map();
  const damageTotals = emptyDamageTotals();
  const packetDamageGroups = new Map();
  let membershipsWithPacketGroupIndex = 0;
  let membershipsWithPacketSkillEffectUuid = 0;
  let membershipsWithPacketTotalDamage = 0;
  let membershipsInMultiComponentPacketGroups = 0;
  let membershipsWithGameTime = 0;
  for (const membership of analysis.memberships) {
    const group = membership.packet_damage_group;
    if (membership.game_time_millis !== null) membershipsWithGameTime += 1;
    if (group?.skill_effect_uuid !== null) membershipsWithPacketSkillEffectUuid += 1;
    if (group?.total_damage_units !== null) membershipsWithPacketTotalDamage += 1;
    if (group?.group_index !== null) {
      membershipsWithPacketGroupIndex += 1;
      const key = JSON.stringify([
        membership.session_id,
        membership.capture_sequence,
        group.group_index,
      ]);
      const retained = packetDamageGroups.get(key) ?? {
        action_ids: new Set(),
        damage_actor_ids: new Set(),
        damage_target_actor_ids: new Set(),
        component_indexes: new Set(),
        component_counts: new Set(),
        memberships: 0,
      };
      retained.action_ids.add(membership.action_id);
      retained.damage_actor_ids.add(membership.damage_actor_id);
      retained.damage_target_actor_ids.add(membership.damage_target_actor_id);
      retained.component_indexes.add(group.component_index);
      retained.component_counts.add(group.component_count);
      retained.memberships += 1;
      packetDamageGroups.set(key, retained);
    }
    if (Number(group?.component_count) > 1) membershipsInMultiComponentPacketGroups += 1;
  }
  const packetGroupRows = [...packetDamageGroups.values()];
  for (const action of analysis.actions) {
    if (action.skill_table_candidates.length === 1) unique += action.damage_memberships;
    else if (action.skill_table_candidates.length > 1) ambiguous += action.damage_memberships;
    else unmapped += action.damage_memberships;
    if (action.owner_id_table_candidates.length === 1) uniqueOwnerTable += action.damage_memberships;
    else if (action.owner_id_table_candidates.length > 1) {
      ambiguousOwnerTable += action.damage_memberships;
    } else unmappedOwnerTable += action.damage_memberships;
    for (const route of action.packet_damage_route_observations ?? []) {
      for (const field of ["reported_amount_units", "hp_loss_units", "shield_loss_units", "actual_amount_units"]) {
        damageTotals[field] += BigInt(route.damage_totals?.[field] ?? "0");
      }
      for (const field of ["events_with_reported_amount", "events_with_hp_loss", "events_with_shield_loss", "events_with_actual_amount"]) {
        damageTotals[field] += Number(route.damage_totals?.[field] ?? 0);
      }
      if (route.resolution === "exact-static-route-selected-by-observed-packet-damage-source") {
        exactPacketRoute += route.damage_memberships;
      } else if (
        route.resolution === "ambiguous-static-route-after-observed-packet-damage-source"
      ) {
        ambiguousPacketRoute += route.damage_memberships;
      } else unresolvedPacketRoute += route.damage_memberships;
      if (
        route.resolution === "exact-static-route-selected-by-observed-packet-damage-source" &&
        route.damage_source === 0
      ) {
        exactSkillRoute += route.damage_memberships;
        stageFamilyEligible += route.damage_memberships;
        if (route.owner_stage !== null) exactSkillRouteWithOwnerStage += route.damage_memberships;
        if (action.skill_table_candidates.length === 1) {
          if (action.skill_table_candidates[0].atk_speed_switch) {
            exactSkillRouteAtkSpeedSwitchTrue += route.damage_memberships;
          } else exactSkillRouteAtkSpeedSwitchFalse += route.damage_memberships;
        }
      }
      if (
        route.resolution === "exact-static-route-selected-by-observed-packet-damage-source" &&
        route.damage_source === 1
      ) {
        exactBulletRoute += route.damage_memberships;
        stageFamilyEligible += route.damage_memberships;
      }
      if (route.damage_source === 0 || route.damage_source === 1) {
        if (route.stage_logic_resolution?.startsWith("exact-current-build-")) {
          exactStageFamily += route.damage_memberships;
          stageTypeCounts.set(route.stage_type, (stageTypeCounts.get(route.stage_type) ?? 0) + route.damage_memberships);
          stageFamilyCounts.set(route.stage_family, (stageFamilyCounts.get(route.stage_family) ?? 0) + route.damage_memberships);
          speedLaneCounts.set(route.speed_lane, (speedLaneCounts.get(route.speed_lane) ?? 0) + route.damage_memberships);
        } else if (route.stage_logic_resolution?.startsWith("ambiguous-")) {
          ambiguousStageFamily += route.damage_memberships;
        } else unresolvedStageFamily += route.damage_memberships;
      }
    }
  }
  return {
    complete_source_side_windows_with_damage: analysis.windows.length,
    source_side_damage_action_memberships: analysis.observedMemberships,
    distinct_damage_action_ids: analysis.actions.length,
    unique_skill_table_candidate_memberships: unique,
    ambiguous_skill_table_candidate_memberships: ambiguous,
    unmapped_skill_table_candidate_memberships: unmapped,
    unique_skill_table_candidate_coverage_parts_per_million:
      analysis.observedMemberships === 0
        ? 0
        : Math.floor((unique * 1_000_000) / analysis.observedMemberships),
    unique_owner_table_candidate_memberships: uniqueOwnerTable,
    ambiguous_owner_table_candidate_memberships: ambiguousOwnerTable,
    unmapped_owner_table_candidate_memberships: unmappedOwnerTable,
    owner_table_candidate_coverage_parts_per_million:
      analysis.observedMemberships === 0
        ? 0
        : Math.floor(
            ((uniqueOwnerTable + ambiguousOwnerTable) * 1_000_000) /
              analysis.observedMemberships,
          ),
    exact_packet_damage_source_selected_route_memberships: exactPacketRoute,
    ambiguous_packet_damage_source_selected_route_memberships: ambiguousPacketRoute,
    unresolved_packet_damage_source_route_memberships: unresolvedPacketRoute,
    packet_damage_source_route_coverage_parts_per_million:
      analysis.observedMemberships === 0
        ? 0
        : Math.floor((exactPacketRoute * 1_000_000) / analysis.observedMemberships),
    exact_skill_route_memberships: exactSkillRoute,
    exact_skill_route_with_owner_stage_memberships: exactSkillRouteWithOwnerStage,
    exact_skill_route_missing_owner_stage_memberships:
      exactSkillRoute - exactSkillRouteWithOwnerStage,
    exact_skill_route_atk_speed_switch_true_memberships: exactSkillRouteAtkSpeedSwitchTrue,
    exact_skill_route_atk_speed_switch_false_memberships: exactSkillRouteAtkSpeedSwitchFalse,
    exact_bullet_route_memberships: exactBulletRoute,
    skill_or_bullet_stage_family_eligible_memberships: stageFamilyEligible,
    exact_action_instance_joins: 0,
    exact_packet_damage_groups: packetDamageGroups.size,
    packet_damage_groups_with_multiple_action_ids: packetGroupRows.filter(
      (group) => group.action_ids.size > 1,
    ).length,
    packet_damage_groups_with_multiple_damage_actors: packetGroupRows.filter(
      (group) => group.damage_actor_ids.size > 1,
    ).length,
    packet_damage_groups_with_multiple_targets: packetGroupRows.filter(
      (group) => group.damage_target_actor_ids.size > 1,
    ).length,
    packet_damage_groups_with_inconsistent_component_count: packetGroupRows.filter(
      (group) => group.component_counts.size !== 1,
    ).length,
    packet_damage_groups_with_duplicate_component_index: packetGroupRows.filter(
      (group) => group.component_indexes.size !== group.memberships,
    ).length,
    completely_retained_packet_damage_groups: packetGroupRows.filter(
      (group) => group.memberships === [...group.component_counts][0],
    ).length,
    partially_retained_packet_damage_groups: packetGroupRows.filter(
      (group) => group.memberships !== [...group.component_counts][0],
    ).length,
    memberships_with_packet_damage_group_index: membershipsWithPacketGroupIndex,
    memberships_with_packet_skill_effect_uuid: membershipsWithPacketSkillEffectUuid,
    memberships_missing_packet_skill_effect_uuid:
      analysis.observedMemberships - membershipsWithPacketSkillEffectUuid,
    memberships_with_packet_total_damage: membershipsWithPacketTotalDamage,
    memberships_in_multi_component_packet_groups: membershipsInMultiComponentPacketGroups,
    memberships_with_interpolated_game_time: membershipsWithGameTime,
    packet_damage_group_is_remote_action_instance: false,
    exact_damage_stage_family_joins: exactStageFamily,
    ambiguous_damage_stage_family_joins: ambiguousStageFamily,
    unresolved_damage_stage_family_joins: unresolvedStageFamily,
    exact_damage_stage_type_memberships: [...stageTypeCounts.entries()]
      .sort((left, right) => Number(left[0]) - Number(right[0]))
      .map(([stage_type, damage_memberships]) => ({ stage_type, damage_memberships })),
    exact_damage_stage_family_memberships: [...stageFamilyCounts.entries()]
      .sort((left, right) => String(left[0]).localeCompare(String(right[0])))
      .map(([stage_family, damage_memberships]) => ({ stage_family, damage_memberships })),
    exact_damage_speed_lane_memberships: [...speedLaneCounts.entries()]
      .sort((left, right) => String(left[0]).localeCompare(String(right[0])))
      .map(([speed_lane, damage_memberships]) => ({ speed_lane, damage_memberships })),
    provider_rdps_credit_allowed: false,
    runtime_promotion_allowed: false,
    observed_damage_reassigned_to_provider: 0,
    ordinary_damage_totals: projectDamageTotals(damageTotals),
  };
}

function validateReport(report) {
  const summary = report?.summary ?? {};
  if (
    ![1, 2, 3, 4, 5, 6, 7, 8, 9, SCHEMA_VERSION].includes(Number(report?.schema_version)) ||
    report?.game_build !== EXPECTED_BUILD ||
    Number(report?.effect_id) !== EXPECTED_EFFECT_ID ||
    report?.relationship_model?.effect_endpoint_damage_role !== "damage_actor" ||
    report?.policy?.remote_player_cast_packets_required !== false ||
    report?.policy?.localized_names_are_runtime_keys !== false ||
    report?.policy?.candidate_skill_table_join_is_formula_authority !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    Number(summary.source_side_damage_action_memberships) !==
      Number(summary.unique_skill_table_candidate_memberships) +
        Number(summary.ambiguous_skill_table_candidate_memberships) +
        Number(summary.unmapped_skill_table_candidate_memberships) ||
    (Number(report.schema_version) >= 2 &&
      (Number(summary.source_side_damage_action_memberships) !==
        Number(summary.unique_owner_table_candidate_memberships) +
          Number(summary.ambiguous_owner_table_candidate_memberships) +
          Number(summary.unmapped_owner_table_candidate_memberships) ||
        Number(summary.owner_table_candidate_coverage_parts_per_million) !== 1_000_000)) ||
    (Number(report.schema_version) >= 3 &&
      Number(summary.source_side_damage_action_memberships) !==
        Number(summary.exact_packet_damage_source_selected_route_memberships) +
          Number(summary.ambiguous_packet_damage_source_selected_route_memberships) +
          Number(summary.unresolved_packet_damage_source_route_memberships)) ||
    (Number(report.schema_version) >= 4 &&
      (Number(summary.exact_skill_route_memberships) !==
        Number(summary.exact_skill_route_with_owner_stage_memberships) +
          Number(summary.exact_skill_route_missing_owner_stage_memberships) ||
        Number(summary.exact_skill_route_memberships) !==
          Number(summary.exact_skill_route_atk_speed_switch_true_memberships) +
            Number(summary.exact_skill_route_atk_speed_switch_false_memberships))) ||
    (Number(report.schema_version) === 5 &&
      (Number(summary.exact_skill_route_memberships) !==
        Number(summary.exact_damage_stage_family_joins) +
          Number(summary.ambiguous_damage_stage_family_joins) +
          Number(summary.unresolved_damage_stage_family_joins) ||
        Number(summary.exact_damage_stage_family_joins) !==
          (summary.exact_damage_stage_type_memberships ?? []).reduce(
            (total, row) => total + Number(row.damage_memberships), 0,
          ) ||
        Number(summary.exact_damage_stage_family_joins) !==
          (summary.exact_damage_stage_family_memberships ?? []).reduce(
            (total, row) => total + Number(row.damage_memberships), 0,
          ) ||
        Number(summary.exact_damage_stage_family_joins) !==
          (summary.exact_damage_speed_lane_memberships ?? []).reduce(
            (total, row) => total + Number(row.damage_memberships), 0,
          ))) ||
    (Number(report.schema_version) >= 6 &&
      (Number(summary.skill_or_bullet_stage_family_eligible_memberships) !==
        Number(summary.exact_skill_route_memberships) + Number(summary.exact_bullet_route_memberships) ||
        Number(summary.skill_or_bullet_stage_family_eligible_memberships) !==
          Number(summary.exact_damage_stage_family_joins) +
            Number(summary.ambiguous_damage_stage_family_joins) +
            Number(summary.unresolved_damage_stage_family_joins) ||
        Number(summary.exact_damage_stage_family_joins) !==
          (summary.exact_damage_stage_type_memberships ?? []).reduce(
            (total, row) => total + Number(row.damage_memberships), 0,
          ) ||
        Number(summary.exact_damage_stage_family_joins) !==
          (summary.exact_damage_stage_family_memberships ?? []).reduce(
            (total, row) => total + Number(row.damage_memberships), 0,
          ) ||
        Number(summary.exact_damage_stage_family_joins) !==
          (summary.exact_damage_speed_lane_memberships ?? []).reduce(
            (total, row) => total + Number(row.damage_memberships), 0,
          ))) ||
    (Number(report.schema_version) >= 7 &&
      (BigInt(summary.ordinary_damage_totals?.reported_amount_units ?? "-1") !==
          (report.action_candidates ?? []).flatMap((action) => action.packet_damage_route_observations ?? [])
            .reduce((total, route) => total + BigInt(route.damage_totals?.reported_amount_units ?? "0"), 0n) ||
        Number(summary.ordinary_damage_totals?.events_with_reported_amount) !==
          Number(summary.source_side_damage_action_memberships))) ||
    (Number(report.schema_version) >= 8 &&
      ((report.damage_action_memberships ?? []).length !==
          Number(summary.source_side_damage_action_memberships) ||
        BigInt(summary.ordinary_damage_totals?.reported_amount_units ?? "-1") !==
          (report.damage_action_memberships ?? []).reduce(
            (total, membership) =>
              total + BigInt(membership.ordinary_damage?.reported_amount_units ?? "0"),
            0n,
          ) ||
        (report.damage_action_memberships ?? []).some(
          (membership) =>
            membership.exact_action_time_speed_state_proven !== false ||
            membership.formula_authority !== false,
        ))) ||
    (Number(report.schema_version) >= 9 &&
      (Number(summary.memberships_with_packet_damage_group_index) !==
          (report.damage_action_memberships ?? []).filter(
            (membership) => membership.packet_damage_group?.group_index !== null,
          ).length ||
        Number(summary.memberships_with_packet_skill_effect_uuid) !==
          (report.damage_action_memberships ?? []).filter(
            (membership) => membership.packet_damage_group?.skill_effect_uuid !== null,
          ).length ||
        Number(summary.memberships_missing_packet_skill_effect_uuid) !==
          Number(summary.source_side_damage_action_memberships) -
            Number(summary.memberships_with_packet_skill_effect_uuid) ||
        summary.packet_damage_group_is_remote_action_instance !== false ||
        (report.damage_action_memberships ?? []).some(
          (membership) =>
            membership.packet_damage_group_is_remote_action_instance !== false ||
            !(
              membership.game_time_millis === null ||
              Number.isSafeInteger(membership.game_time_millis)
            ) ||
            !membership.packet_damage_group ||
            Object.values(membership.packet_damage_group).some(
              (value) => value !== null && typeof value !== "number" && typeof value !== "string"
            ),
        ))) ||
    (Number(report.schema_version) >= 10 &&
      (Number(summary.exact_packet_damage_groups) !==
          Number(summary.completely_retained_packet_damage_groups) +
            Number(summary.partially_retained_packet_damage_groups) ||
        Number(summary.packet_damage_groups_with_inconsistent_component_count) !== 0 ||
        Number(summary.packet_damage_groups_with_duplicate_component_index) !== 0 ||
        Number(summary.packet_damage_groups_with_multiple_action_ids) <= 0 ||
        Number(summary.packet_damage_groups_with_multiple_damage_actors) <= 0 ||
        Number(summary.packet_damage_groups_with_multiple_targets) !== 0)) ||
    Number(summary.exact_action_instance_joins) !== 0 ||
    summary.provider_rdps_credit_allowed !== false ||
    summary.runtime_promotion_allowed !== false ||
    Number(summary.observed_damage_reassigned_to_provider) !== 0 ||
    report.content_sha256 !== contentHash(report)
  ) {
    fail("party-haste damage-skill join proof is inconsistent or unsafe");
  }
}

async function generate(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports exact build ${EXPECTED_BUILD}`);
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const gapBytes = readFileSync(options.gapAudit);
  const gapAudit = JSON.parse(gapBytes);
  validateGapAudit(gapAudit, options.build);
  const skillBytes = readFileSync(options.skillTable);
  const skillTable = JSON.parse(skillBytes);
  const skillEffectBytes = readFileSync(options.skillEffectTable);
  const buffBytes = readFileSync(options.buffTable);
  const bulletBytes = readFileSync(options.bulletTable);
  const damageAttrBytes = readFileSync(options.damageAttrTable);
  const damageSourceRouteBytes = readFileSync(options.damageSourceRouteProof);
  const stageLogicCatalogBytes = readFileSync(options.stageLogicCatalog);
  const skillEffectTable = JSON.parse(skillEffectBytes);
  const buffTable = JSON.parse(buffBytes);
  const bulletTable = JSON.parse(bulletBytes);
  const damageAttrTable = JSON.parse(damageAttrBytes);
  const damageSourceRouteProof = JSON.parse(damageSourceRouteBytes);
  const stageLogicCatalog = parseJsonBytes(stageLogicCatalogBytes);
  const skillIndexes = buildSkillIndexes(skillTable);
  const ownerTableIndexes = buildOwnerTableIndexes({
    skill_table: skillTable,
    skill_effect_table: skillEffectTable,
    buff_table: buffTable,
    bullet_table: bulletTable,
    damage_attr_table: damageAttrTable,
  });
  const damageRouteIndex = buildDamageRouteIndex(damageSourceRouteProof, options.build);
  const stageLogicIndex = buildStageLogicIndex(stageLogicCatalog, options.build);
  const skillEffectIndex = new Map(
    Object.values(skillEffectTable)
      .filter((row) => Number.isSafeInteger(Number(row?.Id)))
      .map((row) => [Number(row.Id), row]),
  );
  const analysis = await analyzeTimeline(
    options.timeline,
    gapAudit,
    skillIndexes,
    ownerTableIndexes,
    damageRouteIndex,
    stageLogicIndex,
    skillEffectIndex,
  );
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-party-haste-damage-skill-join-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    game_build: options.build,
    effect_id: EXPECTED_EFFECT_ID,
    proof_state: "exact-source-side-window-memberships-and-reviewed-skill-or-bullet-stage-families-proven-fail-closed",
    inputs: {
      source_side_gap_window_audit: receipt(options.gapAudit, gapBytes),
      support_timeline: analysis.timeline,
      current_build_skill_table: receipt(options.skillTable, skillBytes),
      current_build_skill_effect_table: receipt(options.skillEffectTable, skillEffectBytes),
      current_build_buff_table: receipt(options.buffTable, buffBytes),
      current_build_bullet_table: receipt(options.bulletTable, bulletBytes),
      current_build_damage_attr_table: receipt(options.damageAttrTable, damageAttrBytes),
      current_build_damage_source_route_proof: receipt(
        options.damageSourceRouteProof,
        damageSourceRouteBytes,
      ),
      current_build_skill_stage_logic_catalog: receipt(
        options.stageLogicCatalog,
        stageLogicCatalogBytes,
      ),
    },
    relationship_model: {
      provider_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      selected_effect_endpoint_damage_role: "damage_actor",
      effect_endpoint_damage_role: "damage_actor",
      target_side_damage_received_is_haste_opportunity: false,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      localized_names_are_evidence_only: true,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_synthesized: false,
      missing_action_instance_is_zero: false,
      absent_packet_skill_effect_uuid_is_zero: false,
      packet_damage_group_is_remote_action_instance: false,
      packet_damage_group_may_mix_action_ids_and_damage_actors: true,
      direct_skill_or_effect_id_namespace_assumed: false,
      owner_id_table_key_presence_is_owner_ancestry: false,
      exact_packet_damage_source_may_select_static_damage_route: true,
      selected_static_damage_route_is_damage_formula_authority: false,
      packet_owner_stage_is_stage_type: false,
      packet_owner_stage_is_zero_based_stage_logic_list_index_after_exact_skill_key_join: true,
      exact_skill_effect_stage_index_join_may_supply_stage_type: true,
      exact_bullet_skill_effect_skill_id_uniform_stage_join_may_supply_speed_lane: true,
      bullet_packet_owner_stage_is_initiating_skill_stage_index: false,
      exact_stage_type_is_action_instance_or_action_time_speed_snapshot: false,
      candidate_skill_table_join_is_formula_authority: false,
      unresolved_actions_retained: true,
      ordinary_damage_totals_unchanged: true,
      provider_rdps_credit_allowed: false,
    },
    bounded_processing: {
      timeline_streamed_one_line_at_a_time: true,
      maximum_retained_windows: analysis.windows.length,
      maximum_retained_action_ids: analysis.actions.length,
      canonical_damage_rows_materialized: true,
      maximum_retained_damage_action_memberships: analysis.memberships.length,
    },
    action_candidates: analysis.actions,
    damage_action_memberships: analysis.memberships,
    summary: summarize(analysis),
    blockers: [
      "the server damage action-ID namespace is not proven per row as direct SkillTable ID versus SkillTable EffectIDs",
      "table-key overlap is resolved only where observed packet damage_source selects one exact current-build route",
      "BuffTable owner IDs still require exact reverse ancestry to their initiating SkillTable rows and stage types",
      "BulletTable owner ID 220203 has no exact SkillEffectTable.SkillId foreign key and remains unresolved",
      "remote damage actions do not carry an exact local action instance or action-time speed snapshot",
      "the retained SkillEffect packet-group identity is exact within one wire message but is not a remote action instance or action clock",
      "exact stage type is static current-build action ancestry, not a remote action instance or action-time speed snapshot",
      "provider-removed action opportunity, operation order, integer rounding, and conservation remain unproven",
      "current-build protocol-pack identity and required replay gates remain missing",
    ],
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    `joined ${report.summary.source_side_damage_action_memberships} source-side memberships; ` +
      `${report.summary.unique_skill_table_candidate_memberships} have one static skill candidate; ` +
      `provider credit=false\nwrote ${options.output}\n`,
  );
}

const options = parseArguments(process.argv.slice(2));
if (options.mode === "verify") {
  const report = JSON.parse(readFileSync(options.input));
  validateReport(report);
  process.stdout.write(
    `verified ${report.summary.source_side_damage_action_memberships} source-side memberships; provider credit=false\n`,
  );
} else {
  await generate(options);
}
