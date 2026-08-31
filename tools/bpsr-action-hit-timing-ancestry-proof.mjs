#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 13;
const EXPECTED_BUILD = "24687926";
const EXPECTED_EFFECT_ID = 31_602;
const RESPONSIVE_LANES = new Set([
  "normal_attack_speed_attr_11720_plus_temporary_700",
  "guide_speed_attr_11730_plus_temporary_710",
]);
const RETAINED_PARAMETERS = new Set([
  "damageAttrId",
  "stageIndex",
  "beginTime",
  "interval",
  "count",
  "damageInterval",
  "damageBegin",
  "damageEnd",
  "ESkillEventType",
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
  const result = {
    command,
    build: take(values, "--build"),
    membership: path.resolve(take(values, "--membership")),
    stageCatalog: path.resolve(take(values, "--stage-catalog")),
    nativeTimingProof: path.resolve(take(values, "--native-timing-proof")),
    output: path.resolve(take(values, "--output")),
  };
  if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
  return result;
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

function parseJsonBytes(bytes) {
  return JSON.parse(bytes.toString("utf8").replace(/^\uFEFF/, ""));
}

function integer(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number)) fail(`${label} must be a safe integer`);
  return number;
}

function parameterIndex(catalog) {
  const index = new Map();
  for (const row of catalog.event_parameter_rows ?? []) {
    const key = String(row.parameter_index);
    const retained = {
      parameter_index: integer(row.parameter_index, "parameter index"),
      name: String(row.param_name),
      type: String(row.param_type),
      value: String(row.param_value),
    };
    const previous = index.get(key);
    if (previous && JSON.stringify(previous) !== JSON.stringify(retained)) {
      fail(`parameter index ${key} has conflicting definitions`);
    }
    index.set(key, retained);
  }
  return index;
}

function buildDamageEventIndex(catalog) {
  const parameters = parameterIndex(catalog);
  const hitIndex = new Map();
  for (const event of catalog.stage_event_rows ?? []) {
    const retained = {};
    for (const parameterId of event.parameter_indexes ?? []) {
      const parameter = parameters.get(String(parameterId));
      if (!parameter) fail(`stage event references missing parameter ${parameterId}`);
      if (!RETAINED_PARAMETERS.has(parameter.name)) continue;
      if (retained[parameter.name] && retained[parameter.name].value !== parameter.value) {
        fail(`stage event has conflicting ${parameter.name} values`);
      }
      retained[parameter.name] = parameter;
    }
    const damageAttrText = retained.damageAttrId?.value;
    if (damageAttrText === undefined || !/^-?\d+$/.test(damageAttrText)) continue;
    const key = `${event.dictionary_kind}:${event.dictionary_key}:${damageAttrText}`;
    const rows = hitIndex.get(key) ?? [];
    rows.push({
      dictionary_kind: String(event.dictionary_kind),
      dictionary_key: integer(event.dictionary_key, "dictionary key"),
      source_skill_id: integer(event.source_skill_id, "source skill ID"),
      event_index: integer(event.event_index, "event index"),
      event_name: String(event.name),
      stage_max_time_raw: String(event.stage_max_time),
      damage_attr_id: damageAttrText,
      stage_index_raw: retained.stageIndex?.value ?? null,
      begin_time_raw: retained.beginTime?.value ?? null,
      interval_raw: retained.interval?.value ?? null,
      count_raw: retained.count?.value ?? null,
      damage_interval_raw: retained.damageInterval?.value ?? null,
      damage_begin_raw: retained.damageBegin?.value ?? null,
      damage_end_raw: retained.damageEnd?.value ?? null,
      skill_event_type_raw: retained.ESkillEventType?.value ?? null,
    });
    hitIndex.set(key, rows);
  }
  return hitIndex;
}

function classifyStageIndex(event, ownerStage) {
  if (event.stage_index_raw === null) return "missing";
  if (!/^-?\d+$/.test(event.stage_index_raw)) return "non_integer";
  return Number(event.stage_index_raw) === ownerStage ? "exact_match" : "mismatch";
}

function parseFiniteFloat32(text) {
  if (text === null || String(text).trim() === "") return null;
  const value = Number(text);
  if (!Number.isFinite(value)) return null;
  return Math.fround(value);
}

function parseSafeInteger(text) {
  if (text === null || !/^-?\d+$/.test(String(text))) return null;
  const value = Number(text);
  return Number.isSafeInteger(value) ? value : null;
}

function float32Evidence(value) {
  const bytes = Buffer.alloc(4);
  bytes.writeFloatLE(value, 0);
  return {
    little_endian_bits_hex: bytes.toString("hex"),
    decimal_diagnostic: value,
  };
}

function standardNativeTiming(event) {
  const eventType = parseSafeInteger(event?.skill_event_type_raw ?? null);
  if (eventType !== 2) {
    return {
      resolution: eventType === 4 ? "unresolved_numeric_motion_type_4" : "unresolved_nonstandard_event_type",
      numeric_event_type: eventType,
      timing: null,
    };
  }
  return {
    resolution: "unresolved_numeric_type_2_native_key_to_catalog_parameter_mapping",
    numeric_event_type: eventType,
    timing: null,
  };
}

function buildGroups(membership, catalog) {
  const hitIndex = buildDamageEventIndex(catalog);
  const groups = new Map();
  let memberships = 0;
  let damage = 0n;
  for (const row of membership.damage_action_memberships ?? []) {
    const route = row.damage_route ?? {};
    if (!RESPONSIVE_LANES.has(route.speed_lane)) continue;
    if ((route.selected_damage_attr_ids ?? []).length !== 1) {
      fail("responsive membership does not select exactly one DamageAttr ID");
    }
    const damageAttrId = String(route.selected_damage_attr_ids[0]);
    const dictionaryKind = String(route.packet_owner_stage_dictionary_kind);
    const dictionaryKey = integer(route.selected_skill_logic_key, "selected logic key");
    const lookupKey = `${dictionaryKind}:${dictionaryKey}:${damageAttrId}`;
    const matches = hitIndex.get(lookupKey) ?? [];
    const event = matches.length === 1 ? matches[0] : null;
    const eventResolution =
      matches.length === 1
        ? "exact_one_damage_bearing_event"
        : matches.length === 0
          ? "unresolved_no_damage_bearing_event"
          : "unresolved_multiple_damage_bearing_events";
    const ownerStage = integer(route.owner_stage, "owner stage");
    const stageIndexResolution = event
      ? classifyStageIndex(event, ownerStage)
      : "unresolved_no_damage_bearing_event";
    const nativeTiming = event
      ? standardNativeTiming(event)
      : { resolution: "unresolved_no_damage_bearing_event", numeric_event_type: null, timing: null };
    const key = JSON.stringify([
      row.action_id,
      dictionaryKind,
      dictionaryKey,
      damageAttrId,
      ownerStage,
      event,
    ]);
    const group = groups.get(key) ?? {
      action_id: integer(row.action_id, "action ID"),
      dictionary_kind: dictionaryKind,
      dictionary_key: dictionaryKey,
      damage_attr_id: damageAttrId,
      packet_owner_stage: ownerStage,
      speed_lane: route.speed_lane,
      damage_event_resolution: eventResolution,
      damage_event: event,
      stage_index_resolution: stageIndexResolution,
      numeric_event_type: nativeTiming.numeric_event_type,
      native_timing_resolution: nativeTiming.resolution,
      native_standard_timing: nativeTiming.timing,
      damage_action_memberships: 0,
      _damage: 0n,
      begin_time_unit_proven:
        nativeTiming.resolution === "exact_numeric_type_2_standard_native_parser_timing",
      interval_unit_proven:
        nativeTiming.resolution === "exact_numeric_type_2_standard_native_parser_timing",
      parser_terminal_formula_proven:
        nativeTiming.resolution === "exact_numeric_type_2_standard_native_parser_timing",
      scheduler_speed_scaling_formula_proven:
        nativeTiming.resolution === "exact_numeric_type_2_standard_native_parser_timing",
      scheduler_speed_value_join_proven: false,
      repetition_schedule_proven: false,
      action_start_to_hit_upper_bound_proven: false,
      formula_authority: false,
      provider_rdps_credit_allowed: false,
    };
    group.damage_action_memberships += 1;
    const reported = BigInt(row.ordinary_damage?.reported_amount_units ?? "0");
    group._damage += reported;
    groups.set(key, group);
    memberships += 1;
    damage += reported;
  }
  return {
    groups: [...groups.values()]
      .map((group) => {
        const reportedDamage = group._damage.toString();
        delete group._damage;
        return { ...group, reported_damage_units: reportedDamage };
      })
      .sort(
        (left, right) =>
          left.action_id - right.action_id ||
          Number(BigInt(left.damage_attr_id) - BigInt(right.damage_attr_id)),
      ),
    memberships,
    damage,
  };
}

function summarize(analysis) {
  const totals = (predicate) => {
    const rows = analysis.groups.filter(predicate);
    return {
      groups: rows.length,
      memberships: rows.reduce((total, row) => total + row.damage_action_memberships, 0),
      damage: rows.reduce((total, row) => total + BigInt(row.reported_damage_units), 0n),
    };
  };
  const exactEvent = totals(
    (row) => row.damage_event_resolution === "exact_one_damage_bearing_event",
  );
  const noEvent = totals(
    (row) => row.damage_event_resolution === "unresolved_no_damage_bearing_event",
  );
  const multipleEvents = totals(
    (row) => row.damage_event_resolution === "unresolved_multiple_damage_bearing_events",
  );
  const beginTime = totals((row) => row.damage_event?.begin_time_raw !== null && row.damage_event);
  const missingBeginTime = totals(
    (row) => row.damage_event !== null && row.damage_event.begin_time_raw === null,
  );
  const stageMatch = totals((row) => row.stage_index_resolution === "exact_match");
  const stageMissing = totals((row) => row.stage_index_resolution === "missing");
  const stageMismatch = totals(
    (row) =>
      row.damage_event !== null &&
      row.stage_index_resolution !== "exact_match" &&
      row.stage_index_resolution !== "missing",
  );
  const repeated = totals((row) => Number(row.damage_event?.count_raw ?? "0") > 1);
  const exactStandardTiming = totals(
    (row) => row.native_timing_resolution === "exact_numeric_type_2_standard_native_parser_timing",
  );
  const unresolvedStandardCatalogMapping = totals(
    (row) =>
      row.native_timing_resolution ===
      "unresolved_numeric_type_2_native_key_to_catalog_parameter_mapping",
  );
  const unresolvedMotion = totals(
    (row) => row.native_timing_resolution === "unresolved_numeric_motion_type_4",
  );
  const unresolvedOtherTiming = totals(
    (row) =>
      row.native_timing_resolution !== "exact_numeric_type_2_standard_native_parser_timing" &&
      row.native_timing_resolution !==
        "unresolved_numeric_type_2_native_key_to_catalog_parameter_mapping" &&
      row.native_timing_resolution !== "unresolved_numeric_motion_type_4",
  );
  return {
    responsive_damage_event_join_groups: analysis.groups.length,
    responsive_damage_event_join_memberships: analysis.memberships,
    responsive_damage_event_join_reported_damage_units: analysis.damage.toString(),
    exact_one_damage_event_match_groups: exactEvent.groups,
    exact_one_damage_event_match_memberships: exactEvent.memberships,
    exact_one_damage_event_match_reported_damage_units: exactEvent.damage.toString(),
    unresolved_no_damage_event_groups: noEvent.groups,
    unresolved_no_damage_event_memberships: noEvent.memberships,
    unresolved_no_damage_event_reported_damage_units: noEvent.damage.toString(),
    unresolved_multiple_damage_event_groups: multipleEvents.groups,
    unresolved_multiple_damage_event_memberships: multipleEvents.memberships,
    unresolved_multiple_damage_event_reported_damage_units: multipleEvents.damage.toString(),
    groups_with_begin_time_parameter: beginTime.groups,
    memberships_with_begin_time_parameter: beginTime.memberships,
    reported_damage_units_with_begin_time_parameter: beginTime.damage.toString(),
    groups_missing_begin_time_parameter: missingBeginTime.groups,
    memberships_missing_begin_time_parameter: missingBeginTime.memberships,
    reported_damage_units_missing_begin_time_parameter: missingBeginTime.damage.toString(),
    groups_with_exact_stage_index_match: stageMatch.groups,
    memberships_with_exact_stage_index_match: stageMatch.memberships,
    groups_missing_stage_index_parameter: stageMissing.groups,
    memberships_missing_stage_index_parameter: stageMissing.memberships,
    groups_with_stage_index_mismatch_or_non_integer: stageMismatch.groups,
    memberships_with_stage_index_mismatch_or_non_integer: stageMismatch.memberships,
    groups_with_no_damage_event_for_stage_index: noEvent.groups,
    memberships_with_no_damage_event_for_stage_index: noEvent.memberships,
    repeated_hit_parameter_groups: repeated.groups,
    repeated_hit_parameter_memberships: repeated.memberships,
    exact_numeric_type_2_standard_timing_groups: exactStandardTiming.groups,
    exact_numeric_type_2_standard_timing_memberships: exactStandardTiming.memberships,
    exact_numeric_type_2_standard_timing_reported_damage_units:
      exactStandardTiming.damage.toString(),
    unresolved_numeric_type_2_catalog_key_mapping_groups:
      unresolvedStandardCatalogMapping.groups,
    unresolved_numeric_type_2_catalog_key_mapping_memberships:
      unresolvedStandardCatalogMapping.memberships,
    unresolved_numeric_type_2_catalog_key_mapping_reported_damage_units:
      unresolvedStandardCatalogMapping.damage.toString(),
    unresolved_numeric_type_4_motion_timing_groups: unresolvedMotion.groups,
    unresolved_numeric_type_4_motion_timing_memberships: unresolvedMotion.memberships,
    unresolved_numeric_type_4_motion_timing_reported_damage_units:
      unresolvedMotion.damage.toString(),
    unresolved_other_timing_groups: unresolvedOtherTiming.groups,
    unresolved_other_timing_memberships: unresolvedOtherTiming.memberships,
    unresolved_other_timing_reported_damage_units: unresolvedOtherTiming.damage.toString(),
    standard_hit_event_to_parser_route_proven: true,
    standard_hitdata_native_timing_formula_proven: true,
    stage_event_parameter_name_to_runtime_dictionary_key_proven: true,
    parser_lookup_global_to_catalog_parameter_identity_proven: false,
    standard_parser_catalog_parameter_mapping_proven: false,
    common_parser_catalog_parameter_mapping_proven: false,
    native_scheduler_speed_scaling_formula_proven: true,
    action_speed_formula_to_scheduler_mechanism_route_proven: true,
    ctrl_skill_callback_registration_and_dispatch_order_proven: true,
    action_speed_formula_native_sampling_point_proven: true,
    companion_factor_component_identity_proven: true,
    battle_frame_component_reference_surface_proven: true,
    computed_battle_frame_setter_path_proven: true,
    battle_frame_globally_constant_proven: false,
    computed_setter_host_preview_only_proven: false,
    exact_float32_battle_frame_cancellation_authorized: false,
    exact_scheduler_speed_value_join_memberships: 0,
    effective_speed_scaled_timing_materialized: false,
    begin_time_unit_proven: false,
    interval_unit_proven: false,
    repetition_schedule_proven: false,
    action_start_to_hit_upper_bound_proven: false,
    provider_rdps_credit_allowed: false,
    ui_rdps_display_allowed: false,
    runtime_promotion_allowed: false,
    observed_damage_reassigned_to_provider: 0,
  };
}

function validateReport(report) {
  const groups = report?.action_damage_event_timing_groups ?? [];
  const summary = report?.summary ?? {};
  const memberships = groups.reduce(
    (total, group) => total + Number(group.damage_action_memberships),
    0,
  );
  const damage = groups.reduce(
    (total, group) => total + BigInt(group.reported_damage_units),
    0n,
  );
  if (
    ![3, 4, 5, 6, 7, 8, SCHEMA_VERSION].includes(Number(report?.schema_version)) ||
    report?.game_build !== EXPECTED_BUILD ||
    Number(report?.effect_id) !== EXPECTED_EFFECT_ID ||
    Number(summary.responsive_damage_event_join_memberships) !== memberships ||
    BigInt(summary.responsive_damage_event_join_reported_damage_units ?? "-1") !== damage ||
    Number(summary.exact_one_damage_event_match_memberships) +
        Number(summary.unresolved_no_damage_event_memberships) +
        Number(summary.unresolved_multiple_damage_event_memberships) !==
      memberships ||
    Number(summary.memberships_with_begin_time_parameter) +
        Number(summary.memberships_missing_begin_time_parameter) !==
      Number(summary.exact_one_damage_event_match_memberships) ||
    Number(summary.memberships_with_exact_stage_index_match) +
        Number(summary.memberships_missing_stage_index_parameter) +
        Number(summary.memberships_with_stage_index_mismatch_or_non_integer) +
        Number(summary.memberships_with_no_damage_event_for_stage_index) !==
      memberships ||
    Number(summary.exact_numeric_type_2_standard_timing_memberships) +
        Number(summary.unresolved_numeric_type_2_catalog_key_mapping_memberships) +
        Number(summary.unresolved_numeric_type_4_motion_timing_memberships) +
        Number(summary.unresolved_other_timing_memberships) !==
      memberships ||
    summary.standard_hit_event_to_parser_route_proven !== true ||
    summary.standard_hitdata_native_timing_formula_proven !== true ||
    summary.stage_event_parameter_name_to_runtime_dictionary_key_proven !== true ||
    summary.parser_lookup_global_to_catalog_parameter_identity_proven !== false ||
    (Number(report.schema_version) >= 10 &&
      (summary.standard_parser_catalog_parameter_mapping_proven !== false ||
        summary.common_parser_catalog_parameter_mapping_proven !== false ||
        Number(summary.exact_numeric_type_2_standard_timing_memberships) !== 0)) ||
    (Number(report.schema_version) >= 5 &&
      (summary.native_scheduler_speed_scaling_formula_proven !== true ||
        Number(summary.exact_scheduler_speed_value_join_memberships) !== 0 ||
        summary.effective_speed_scaled_timing_materialized !== false)) ||
    (Number(report.schema_version) >= 6 &&
      summary.action_speed_formula_to_scheduler_mechanism_route_proven !== true) ||
    (Number(report.schema_version) >= 7 &&
      (summary.ctrl_skill_callback_registration_and_dispatch_order_proven !== true ||
        summary.action_speed_formula_native_sampling_point_proven !== true)) ||
    (Number(report.schema_version) >= 8 &&
      summary.companion_factor_component_identity_proven !== true) ||
    (Number(report.schema_version) >= 9 &&
      (summary.battle_frame_component_reference_surface_proven !== true ||
        summary.exact_float32_battle_frame_cancellation_authorized !== false)) ||
    (Number(report.schema_version) >= 13 &&
      (summary.computed_battle_frame_setter_path_proven !== true ||
        summary.battle_frame_globally_constant_proven !== false ||
        summary.computed_setter_host_preview_only_proven !== false)) ||
    summary.action_start_to_hit_upper_bound_proven !== false ||
    summary.provider_rdps_credit_allowed !== false ||
    summary.ui_rdps_display_allowed !== false ||
    (Number(report.schema_version) >= 4 &&
      (report?.damage_packet_container_evidence?.packet_damage_group_is_remote_action_instance !==
          false ||
        Number(
          report?.damage_packet_container_evidence
            ?.packet_damage_groups_with_multiple_action_ids,
        ) <= 0 ||
        Number(
          report?.damage_packet_container_evidence
            ?.packet_damage_groups_with_multiple_damage_actors,
        ) <= 0 ||
        Number(
          report?.damage_packet_container_evidence
            ?.memberships_with_packet_skill_effect_uuid,
        ) !== 0)) ||
    Number(summary.observed_damage_reassigned_to_provider) !== 0 ||
    groups.some(
      (group) =>
        group.action_start_to_hit_upper_bound_proven !== false ||
        group.formula_authority !== false ||
        group.provider_rdps_credit_allowed !== false ||
        (group.native_timing_resolution ===
          "exact_numeric_type_2_standard_native_parser_timing" &&
          (group.begin_time_unit_proven !== true ||
            group.interval_unit_proven !== true ||
            group.parser_terminal_formula_proven !== true ||
            (Number(report.schema_version) >= 5 &&
              (group.scheduler_speed_scaling_formula_proven !== true ||
                group.scheduler_speed_value_join_proven !== false ||
                group.native_standard_timing?.effective_scaled_timing !== null)) ||
            group.native_standard_timing === null)),
    ) ||
    report.content_sha256 !== contentHash(report)
  ) {
    fail("action damage-event timing ancestry proof is inconsistent or unsafe");
  }
}

function generate(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports build ${EXPECTED_BUILD}`);
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const membershipBytes = readFileSync(options.membership);
  const catalogBytes = readFileSync(options.stageCatalog);
  const nativeTimingBytes = readFileSync(options.nativeTimingProof);
  const membership = parseJsonBytes(membershipBytes);
  const catalog = parseJsonBytes(catalogBytes);
  const nativeTiming = parseJsonBytes(nativeTimingBytes);
  if (
    ![8, 9, 10].includes(Number(membership?.schema_version)) ||
    membership?.game_build !== options.build ||
    Number(membership?.effect_id) !== EXPECTED_EFFECT_ID ||
    membership?.policy?.provider_rdps_credit_allowed !== false ||
    Number(catalog?.schema_version) !== 3 ||
    String(catalog?.build) !== options.build ||
    catalog?.summary?.unresolved_stage_event_parameter_references !== 0 ||
    Number(nativeTiming?.schema_version) !== 11 ||
    nativeTiming?.game_build !== options.build ||
    nativeTiming?.summary?.standard_hit_event_to_parser_route_proven !== true ||
    nativeTiming?.summary?.standard_hitdata_native_timing_formula_proven !== true ||
    nativeTiming?.summary?.stage_event_parameter_name_to_runtime_dictionary_key_proven !== true ||
    nativeTiming?.summary?.parser_lookup_global_to_catalog_parameter_identity_proven !== false ||
    nativeTiming?.summary?.standard_parser_catalog_parameter_mapping_proven !== false ||
    nativeTiming?.summary?.common_parser_catalog_parameter_mapping_proven !== false ||
    nativeTiming?.summary?.wrapper_speed_to_scheduler_parameter_proven !== true ||
    nativeTiming?.summary?.time_factor_to_outgoing_hit_scheduler_proven !== true ||
    nativeTiming?.summary?.scheduler_speed_scaling_formula_proven !== true ||
    nativeTiming?.summary?.action_speed_formula_to_scheduler_mechanism_route_proven !== true ||
    nativeTiming?.summary?.ctrl_skill_callback_registration_and_dispatch_order_proven !== true ||
    nativeTiming?.summary?.action_speed_formula_native_sampling_point_proven !== true ||
    nativeTiming?.summary?.companion_factor_component_identity_proven !== true ||
    nativeTiming?.summary?.battle_frame_component_reference_surface_proven !== true ||
    nativeTiming?.summary?.computed_battle_frame_setter_path_proven !== true ||
    nativeTiming?.summary?.battle_frame_globally_constant_proven !== false ||
    nativeTiming?.summary?.computed_setter_host_preview_only_proven !== false ||
    nativeTiming?.summary?.exact_float32_battle_frame_cancellation_authorized !== false ||
    nativeTiming?.summary?.action_speed_formula_to_each_scheduler_invocation_proven !== false ||
    nativeTiming?.summary?.provider_rdps_credit_allowed !== false
  ) {
    fail("inputs are not the exact current-build membership and stage-event frontier");
  }
  const analysis = buildGroups(membership, catalog);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-action-hit-timing-ancestry-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    game_build: options.build,
    effect_id: EXPECTED_EFFECT_ID,
    proof_state:
      "native-parser-and-scheduler-formulas-proven-catalog-key-mapping-observed-speed-value-motion-and-packet-clock-joins-open",
    inputs: {
      damage_action_membership_ledger: receipt(options.membership, membershipBytes),
      current_build_stage_logic_catalog: receipt(options.stageCatalog, catalogBytes),
      current_build_native_timing_proof: receipt(options.nativeTimingProof, nativeTimingBytes),
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      remote_player_cast_packets_required: false,
      exact_numeric_skill_event_type_is_runtime_route_authority: true,
      localized_or_display_event_names_are_runtime_route_authority: false,
      missing_timing_parameters_are_zero: false,
      similarly_named_catalog_parameters_are_native_lookup_keys: false,
      packet_damage_group_is_remote_action_instance: false,
      interpolated_game_time_is_packet_damage_action_timestamp: false,
      ordinary_damage_totals_unchanged: true,
      provider_rdps_credit_allowed: false,
      ui_rdps_display_allowed: false,
    },
    action_damage_event_timing_groups: analysis.groups,
    damage_packet_container_evidence: {
      exact_packet_damage_groups: Number(membership.summary.exact_packet_damage_groups),
      packet_damage_groups_with_multiple_action_ids: Number(
        membership.summary.packet_damage_groups_with_multiple_action_ids,
      ),
      packet_damage_groups_with_multiple_damage_actors: Number(
        membership.summary.packet_damage_groups_with_multiple_damage_actors,
      ),
      memberships_with_packet_skill_effect_uuid: Number(
        membership.summary.memberships_with_packet_skill_effect_uuid,
      ),
      memberships_missing_packet_skill_effect_uuid: Number(
        membership.summary.memberships_missing_packet_skill_effect_uuid,
      ),
      memberships_with_interpolated_game_time: Number(
        membership.summary.memberships_with_interpolated_game_time,
      ),
      packet_damage_group_is_remote_action_instance: false,
      interpolated_game_time_is_packet_damage_action_timestamp: false,
    },
    summary: summarize(analysis),
    blockers: [
      "native standard-parser arithmetic is proven, but exact native lookup keys are not joined to current-build catalog parameter identities; the 3,367 formerly materialized type-2 memberships are now fail-closed unresolved",
      "numeric ESkillEventType 4 motion timing remains unresolved pending exact common-parser key and unit mapping",
      "a damage-bearing event without beginTime or stageIndex remains exact ancestry but unresolved timing",
      "the standard parser terminal is proven but its relationship to the final damage occurrence remains unproven",
      "the native event clock is not yet joined to the observed damage packet clock with a transport latency bound",
      "the SkillEffect packet container mixes action IDs and damage actors and cannot replace a remote action instance",
      "the native CtrlSkill lifecycle and LocalAttrBattleFrameSpeed write surface are proven, including a computed float32 setter path whose affected host is not proven preview-only; exact float32 composition does not permit algebraic cancellation without the per-action companion or composed speed value",
      "current-build protocol-pack identity and required replay gates remain missing",
    ],
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    `partitioned ${analysis.memberships} responsive memberships into ${analysis.groups.length} damage-event timing groups; timing authority=false\nwrote ${options.output}\n`,
  );
}

const options = parseArguments(process.argv.slice(2));
if (options.command === "generate") generate(options);
else {
  const report = JSON.parse(readFileSync(options.input));
  validateReport(report);
  process.stdout.write(
    `verified ${report.summary.responsive_damage_event_join_memberships} damage-event timing memberships; provider credit=false\n`,
  );
}
