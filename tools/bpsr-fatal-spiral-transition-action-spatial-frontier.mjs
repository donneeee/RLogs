#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 3;
const GENERATED_BY = "tools/bpsr-fatal-spiral-transition-action-spatial-frontier.mjs";
const GAME_BUILD = "24687926";
const EFFECT_ID = 2110125;

const EXPECTED_SELECTORS = new Map([
  ["ability=2295;hit_event=2;owner=2295;property=7;damage_source=none;damage_type=none;damage_mode=1;normal_hit=true;passive_uuid=none", { count: 7, lookupKey: "2295:2" }],
  ["ability=2352;hit_event=3;owner=2352;property=7;damage_source=none;damage_type=none;damage_mode=1;normal_hit=true;passive_uuid=none", { count: 36, lookupKey: "2352:3" }],
  ["ability=55240;hit_event=3;owner=55240;property=7;damage_source=2;damage_type=none;damage_mode=1;normal_hit=true;passive_uuid=none", { count: 15, lookupKey: "55240:3" }],
  ["ability=55240;hit_event=4;owner=55240;property=7;damage_source=2;damage_type=none;damage_mode=1;normal_hit=true;passive_uuid=none", { count: 5, lookupKey: "55240:4" }],
  ["ability=220101;hit_event=none;owner=220101;property=7;damage_source=4;damage_type=none;damage_mode=1;normal_hit=true;passive_uuid=none", { count: 1, lookupKey: "220101:0" }],
  ["ability=2203521;hit_event=5;owner=2203521;property=7;damage_source=2;damage_type=none;damage_mode=1;normal_hit=true;passive_uuid=none", { count: 4, lookupKey: "2203521:5" }],
  ["ability=2203531;hit_event=1;owner=2203531;property=7;damage_source=2;damage_type=none;damage_mode=1;normal_hit=true;passive_uuid=none", { count: 1, lookupKey: "2203531:1" }],
]);

const TABLE_MANIFEST_IDS = {
  skillTable: "decoded-game-tables:SkillTable.json",
  skillEffectTable: "decoded-game-tables:SkillEffectTable.json",
  bulletTable: "decoded-game-tables:BulletTable.json",
  buffTable: "decoded-game-tables:BuffTable.json",
  damageAttrTable: "decoded-game-tables:DamageAttrTable.json",
};

function fail(message) {
  throw new Error(message);
}

function readJson(file, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    fail(`Cannot read ${label} ${file}: ${error.message}`);
  }
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(canonical(copy)).digest("hex").toUpperCase();
}

function descriptor(file) {
  const bytes = fs.readFileSync(file);
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function parseIdentity(identity) {
  return Object.fromEntries(identity.split(";").map((part) => {
    const split = part.indexOf("=");
    if (split <= 0) fail(`Invalid action identity component ${part}`);
    return [part.slice(0, split), part.slice(split + 1)];
  }));
}

function selectorIdentity(parts) {
  return ["ability", "hit_event", "owner", "property", "damage_source", "damage_type", "damage_mode", "normal_hit", "passive_uuid"]
    .map((key) => `${key}=${parts[key]}`)
    .join(";");
}

function numericOrNull(value) {
  return value === "none" ? null : Number(value);
}

function enumValues(combatSurface, name) {
  const type = combatSurface.types?.find((entry) => entry.namespace === "Zproto" && entry.name === name);
  if (type?.kind !== "enum") fail(`Exact-build ${name} enum is missing`);
  return Object.fromEntries(type.enum_values.map((entry) => [entry.name, Number(entry.value)]));
}

function validateTransition(transition) {
  const summary = transition.summary ?? {};
  if (
    transition.schema_version !== 9 ||
    transition.generated_by !== "rlogs-bpsr-rlog-transition-counterfactual-audit" ||
    transition.game_build !== GAME_BUILD ||
    Number(transition.effect_id) !== EFFECT_ID ||
    transition.damage_relationship !== "source" ||
    transition.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    transition.policy?.relative_spatial_relations_are_diagnostic_only !== true ||
    transition.policy?.candidate_pairs_never_grant_formula_or_runtime_authority !== true ||
    Number(summary.configured_endpoint_transition_pairs) !== 69 ||
    Number(summary.strict_controlled_counterfactual_pairs) !== 0 ||
    summary.formula_authority !== false ||
    summary.provider_rdps_credit_allowed !== false ||
    !/^sha256:[0-9a-f]{64}$/.test(transition.content_sha256 ?? "")
  ) fail("Transition audit is unsafe, incompatible, or has an invalid digest");
  const relations = summary.configured_endpoint_transition_spatial_relations ?? {};
  const direct = relations.source_attr_pos_to_target_attr_pos ?? {};
  const targetMotion = relations.target_attr_pos_to_target_attr_target_pos ?? {};
  if (
    Object.keys(relations).length !== 6 ||
    Object.values(relations).some((relation) => Number(relation.pair_count) !== 69 ||
      relation.spatial_state_safe_to_exclude_from_counterfactual_matching !== false ||
      relation.formula_authority !== false) ||
    Number(direct.both_relations_complete_count) !== 66 ||
    Number(direct.exact_displacement_vector_equal_count) !== 2 ||
    Number(direct.exact_squared_distance_equal_count) !== 2 ||
    Number(direct.absolute_distance_delta_le_1_count) !== 54 ||
    Number(targetMotion.both_relations_complete_count) !== 66 ||
    Number(targetMotion.exact_displacement_vector_equal_count) !== 59
  ) fail("Transition relative-spatial frontier is unsafe or inconsistent");
}

function validateBuildManifest(manifest, tableDescriptors) {
  if (
    manifest.schemaVersion !== 1 ||
    String(manifest.gameBuild) !== GAME_BUILD ||
    manifest.authority?.decodedGameTables !== "exact-current-build-static-data" ||
    manifest.authority?.packetReplay !== "runtime-behavior-proof" ||
    (manifest.missingRequiredFiles ?? []).length !== 0 ||
    (manifest.files ?? []).length !== 693
  ) fail("Complete-build source manifest is unsafe or incompatible");
  for (const [role, manifestId] of Object.entries(TABLE_MANIFEST_IDS)) {
    const entry = manifest.files.find((candidate) => candidate.id === manifestId);
    const input = tableDescriptors[role];
    if (
      !entry || entry.authority !== "exact-current-build-static-data" ||
      Number(entry.bytes) !== input.bytes || entry.sha256 !== input.sha256
    ) fail(`${role} does not match the exact-build source manifest`);
  }
}

function validateResearchInputs(ledger, inventory, combatSurface) {
  const sources = enumValues(combatSurface, "EDamageSource");
  if (
    ledger.schema_version !== 2 || ledger.game_build !== GAME_BUILD ||
    ledger.generated_by !== "rlogs-bpsr-damage-resolution-ledger" ||
    ledger.policy?.packet_source_and_recount_parent_are_separate !== true ||
    ledger.policy?.static_formula_is_runtime_authority !== false ||
    ledger.policy?.unresolved_evidence_hidden !== false ||
    inventory.schema_version !== 1 || inventory.game_build !== GAME_BUILD ||
    inventory.generated_by !== "rlogs-bpsr-damage-script-static-input-inventory" ||
    inventory.policy?.runtime_formula_authority !== false ||
    inventory.policy?.server_operator_implementation_present !== false ||
    inventory.policy?.static_field_values_are_formula_operators !== false ||
    combatSurface.schema_version !== 2 || combatSurface.build_id !== GAME_BUILD ||
    sources.EDamageSourceSkill !== 0 || sources.EDamageSourceBullet !== 1 ||
    sources.EDamageSourceBuff !== 2 || sources.EDamageSourceFall !== 3 ||
    sources.EDamageSourceFakeBullet !== 4 || sources.EDamageSourceOther !== 100
  ) fail("Current-build research inputs are unsafe or incompatible");
  return sources;
}

function validateFakeBulletLifecycle(frontier) {
  if (
    frontier.schema_version !== 1 ||
    frontier.generated_by !== "tools/bpsr-fake-bullet-lifecycle-frontier.mjs" ||
    String(frontier.game_build) !== GAME_BUILD ||
    Number(frontier.exact_wire_contract?.damage_source_id) !== 4 ||
    frontier.exact_wire_contract?.damage_source_name !== "EDamageSourceFakeBullet" ||
    frontier.exact_wire_contract?.exact_build_wire_authority !== true ||
    Number(frontier.exact_wire_contract?.fields?.length) !== 7 ||
    Number(frontier.canonical_timeline_contract?.event_schema_version) !== 9 ||
    frontier.canonical_timeline_contract?.event_kind !== "unresolved_action" ||
    frontier.canonical_timeline_contract?.container_entity_is_named_provider !== false ||
    frontier.canonical_timeline_contract?.raw_payload_retained !== true ||
    frontier.canonical_timeline_contract?.ordinary_damage_event_synthesized !== false ||
    Number(frontier.observed_evidence_frontier?.current_build_observed_action_220101_fake_bullet_lifecycle_records) !== 0 ||
    frontier.observed_evidence_frontier?.historical_canonical_logs_backfilled !== false ||
    frontier.observed_evidence_frontier?.future_captures_can_preserve_exact_join_keys !== true ||
    frontier.observed_evidence_frontier?.enclosing_aoi_entity_is_provider_proven !== false ||
    frontier.observed_evidence_frontier?.source4_to_damage_component_join_proven !== false ||
    frontier.policy?.provider_rdps_credit_allowed !== false ||
    frontier.content_sha256 !== digest(frontier)
  ) fail("Fake-bullet lifecycle frontier is unsafe or incompatible");
}

function staticSurface(route, abilityId, tables) {
  if (route?.damage_source === "skill") {
    const skill = tables.skillTable[String(abilityId)];
    const effect = tables.skillEffectTable[String(route.intermediary_id)];
    if (!skill || !effect || Number(effect.SkillId) !== abilityId) fail(`Missing exact skill surface for ${abilityId}`);
    return {
      source_table: "SkillTable + SkillEffectTable",
      skill: {
        id: Number(skill.Id), effect_ids: skill.EffectIDs ?? [], target_type: skill.TargetType,
        target_range_type: skill.SkillTargetRangeType, range_type: skill.SkillRangeType,
        select_point_type: skill.SkillSelectPointType, is_aoe: skill.IsAoe, searches_enemies: skill.IsSearchEnemie,
      },
      skill_effect: {
        id: Number(effect.Id), skill_id: Number(effect.SkillId), damage_distance: effect.SkillDamageDistance,
        effect_range: effect.EffectRange ?? [], max_horizontal_motion_distance: effect.MaxHorizontalMotionDis,
        single_target_range: effect.STRange ?? [],
      },
    };
  }
  if (route?.damage_source === "bullet") {
    const bullet = tables.bulletTable[String(abilityId)];
    if (!bullet) fail(`Missing exact bullet surface for ${abilityId}`);
    const effect = tables.skillEffectTable[String(abilityId)] ?? null;
    return {
      source_table: "BulletTable" + (effect ? " + SkillEffectTable" : ""),
      bullet: {
        id: Number(bullet.Id), bullet_attr_id: Number(bullet.BulletAttrId), bullet_type: bullet.BulletType,
        is_follow: bullet.IsFollow, duration: bullet.Duration, damage_weight: bullet.DamageWeight,
        damage_weight_type: bullet.DamageWeightType, hit_camp_type: bullet.HitCampType ?? [],
      },
      skill_effect: effect ? {
        id: Number(effect.Id), skill_id: Number(effect.SkillId), damage_distance: effect.SkillDamageDistance,
        effect_range: effect.EffectRange ?? [], max_horizontal_motion_distance: effect.MaxHorizontalMotionDis,
        single_target_range: effect.STRange ?? [],
      } : null,
    };
  }
  if (route?.damage_source === "buff") {
    const buff = tables.buffTable[String(abilityId)];
    if (!buff) fail(`Missing exact buff surface for ${abilityId}`);
    return {
      source_table: "BuffTable",
      buff: {
        id: Number(buff.Id), duration: buff.Duration ?? null, repeat: buff.Repeat ?? null,
        special_attributes: buff.SpecialAttr ?? [], tags: buff.Tags ?? [],
      },
    };
  }
  fail(`Unsupported static route ${route?.damage_source ?? "<missing>"}`);
}

function build(options) {
  const transition = readJson(options.transitionAudit, "transition audit");
  const manifest = readJson(options.buildManifest, "complete-build source manifest");
  const ledger = readJson(options.damageLedger, "damage-resolution ledger");
  const inventory = readJson(options.damageScriptInventory, "damage-script inventory");
  const combatSurface = readJson(options.combatSurface, "IL2CPP combat surface");
  const fakeBulletLifecycle = readJson(options.fakeBulletLifecycle, "fake-bullet lifecycle frontier");
  const tableFiles = {
    skillTable: options.skillTable,
    skillEffectTable: options.skillEffectTable,
    bulletTable: options.bulletTable,
    buffTable: options.buffTable,
    damageAttrTable: options.damageAttrTable,
  };
  const tableDescriptors = Object.fromEntries(Object.entries(tableFiles).map(([role, file]) => [role, descriptor(file)]));
  const tables = Object.fromEntries(Object.entries(tableFiles).map(([role, file]) => [role, readJson(file, role)]));

  validateTransition(transition);
  validateBuildManifest(manifest, tableDescriptors);
  const damageSourceEnum = validateResearchInputs(ledger, inventory, combatSurface);
  validateFakeBulletLifecycle(fakeBulletLifecycle);

  const grouped = new Map();
  for (const [identity, countValue] of Object.entries(transition.summary.configured_endpoint_transition_action_identity_counts ?? {})) {
    const parts = parseIdentity(identity);
    const selector = selectorIdentity(parts);
    const group = grouped.get(selector) ?? { count: 0, component_identities: [] };
    const count = Number(countValue);
    group.count += count;
    group.component_identities.push({ identity, count });
    grouped.set(selector, group);
  }
  if (grouped.size !== EXPECTED_SELECTORS.size) fail(`Expected ${EXPECTED_SELECTORS.size} action selectors, found ${grouped.size}`);

  const actions = [];
  for (const [selector, expected] of EXPECTED_SELECTORS) {
    const group = grouped.get(selector);
    if (!group || group.count !== expected.count) fail(`Unexpected transition count for ${selector}`);
    const packet = parseIdentity(selector);
    const ledgerEntry = ledger.entries?.find((entry) => entry.lookup_key === expected.lookupKey);
    if (!ledgerEntry || ledgerEntry.formula?.state !== "standard-static-candidate" || ledgerEntry.readiness !== "runtime-replay-ready") {
      fail(`Missing candidate ledger entry ${expected.lookupKey}`);
    }
    const packetSourceWireValue = numericOrNull(packet.damage_source);
    const packetSourceEffectiveValue = packetSourceWireValue ?? damageSourceEnum.EDamageSourceSkill;
    const matchingRoute = ledgerEntry.source?.routes?.find((route) => Number(route.damage_source_id) === packetSourceEffectiveValue) ?? null;
    const candidateRoute = ledgerEntry.source?.routes?.[0] ?? null;
    const formula = ledgerEntry.formula.rule;
    const damageAttr = tables.damageAttrTable[String(ledgerEntry.damage_attr_id)];
    if (
      !candidateRoute || !damageAttr || Number(damageAttr.Id) !== Number(ledgerEntry.damage_attr_id) ||
      damageAttr.DamageScript !== formula.damage_script || Number(damageAttr.DamageProperty) !== Number(formula.damage_property)
    ) fail(`Damage table and ledger disagree for ${expected.lookupKey}`);

    actions.push({
      selector,
      observed_transition_pairs: group.count,
      component_identities: group.component_identities.sort((a, b) => a.identity.localeCompare(b.identity)),
      packet_identity: {
        ability_id: Number(packet.ability), hit_event_id: numericOrNull(packet.hit_event), owner_id: Number(packet.owner),
        property: Number(packet.property), damage_source_wire_value: packetSourceWireValue,
        damage_source_effective_enum_value: packetSourceEffectiveValue,
        damage_source_effective_enum_name: Object.entries(damageSourceEnum).find(([, value]) => value === packetSourceEffectiveValue)?.[0] ?? null,
        damage_type: numericOrNull(packet.damage_type), damage_mode: Number(packet.damage_mode),
        normal_hit: packet.normal_hit === "true", passive_uuid: numericOrNull(packet.passive_uuid),
      },
      static_route: {
        lookup_key: expected.lookupKey,
        candidate_damage_attr_id: Number(ledgerEntry.damage_attr_id),
        candidate_route: candidateRoute,
        packet_source_matching_route: matchingRoute,
        packet_source_matches_candidate_route: matchingRoute !== null,
        state: matchingRoute ? "exact-current-build-packet-source-match" : "rejected-packet-source-mismatch",
      },
      formula_candidate: {
        selected_for_counterfactual_evaluation: matchingRoute !== null,
        state: matchingRoute ? "static-candidate-operator-and-replay-proof-required" : "unresolved-no-packet-source-compatible-static-route",
        damage_attr_id: Number(ledgerEntry.damage_attr_id), damage_script: formula.damage_script,
        coefficient_basis_points_by_stage: formula.coefficient_basis_points_by_stage,
        fixed_parameter_by_level: formula.fixed_parameter_by_level,
        damage_property: formula.damage_property, damage_weight: formula.damage_weight, tags: formula.tags,
        server_operator_implementation_proven: false,
      },
      static_spatial_surface: staticSurface(candidateRoute, Number(packet.ability), tables),
      proof_state: {
        exact_build_table_identity_proven: true,
        packet_source_route_proven: matchingRoute !== null,
        component_index_is_formula_input: false,
        static_zero_or_empty_spatial_fields_prove_position_independence: false,
        separate_range_attenuation_script_names_prove_attack_position_independence: false,
        spatial_state_safe_to_exclude_from_counterfactual_matching: false,
        operation_order_proven: false,
        integer_rounding_proven: false,
        formula_authority: false,
        ui_display_authority: false,
        provider_rdps_credit_allowed: false,
      },
    });
  }

  actions.sort((a, b) => a.selector.localeCompare(b.selector));
  const fakeBulletAction = actions.find((action) => action.packet_identity.ability_id === 220101);
  if (
    !fakeBulletAction ||
    fakeBulletAction.packet_identity.damage_source_effective_enum_value !== 4 ||
    fakeBulletAction.static_route.state !== "rejected-packet-source-mismatch"
  ) fail("Expected unresolved source-4 action 220101 is missing");
  fakeBulletAction.unresolved_lifecycle = {
    canonical_event_kind: fakeBulletLifecycle.canonical_timeline_contract.event_kind,
    exact_wire_join_keys_preserved_for_future_captures: true,
    current_build_observed_lifecycle_records:
      fakeBulletLifecycle.observed_evidence_frontier.current_build_observed_action_220101_fake_bullet_lifecycle_records,
    historical_canonical_logs_backfilled: false,
    enclosing_aoi_entity_is_provider_proven: false,
    source4_to_damage_component_join_proven: false,
    provider_rdps_credit_allowed: false,
  };
  const matchingPairs = actions.reduce((sum, action) => sum + (action.static_route.packet_source_matches_candidate_route ? action.observed_transition_pairs : 0), 0);
  const rejectedPairs = actions.reduce((sum, action) => sum + (!action.static_route.packet_source_matches_candidate_route ? action.observed_transition_pairs : 0), 0);
  const attenuationFamilies = inventory.families.filter((family) => ["AttackRangeAttenuation", "MAttackRangeAttenuation"].includes(family.damage_script));
  const spatialRelations = transition.summary.configured_endpoint_transition_spatial_relations;
  const directSpatialRelation = spatialRelations.source_attr_pos_to_target_attr_pos;
  if (matchingPairs !== 68 || rejectedPairs !== 1 || attenuationFamilies.length !== 2) fail("Unexpected action route or attenuation-family frontier");

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    effect_id: EFFECT_ID,
    damage_relationship: "source",
    inputs: {
      transition_audit: descriptor(options.transitionAudit),
      complete_build_source_manifest: descriptor(options.buildManifest),
      damage_resolution_ledger: descriptor(options.damageLedger),
      damage_script_static_input_inventory: descriptor(options.damageScriptInventory),
      il2cpp_combat_surface: descriptor(options.combatSurface),
      fake_bullet_lifecycle_frontier: descriptor(options.fakeBulletLifecycle),
      exact_build_tables: tableDescriptors,
    },
    source_identity: {
      complete_build_manifest_aggregate_sha256: manifest.aggregateSha256,
      exact_build_table_manifest_bindings_complete: true,
      damage_source_enum: damageSourceEnum,
    },
    policy: {
      exact_numeric_ids_are_runtime_keys: true,
      localized_names_are_runtime_keys: false,
      structurally_unobservable_remote_player_cast_packets_are_required: false,
      absent_packet_damage_source_is_semantic_enum_default_zero_not_packet_absence_as_damage_zero: true,
      component_index_and_count_are_packet_segmentation_not_formula_identity: true,
      static_table_values_are_server_operator_proof: false,
      static_zero_or_empty_spatial_fields_are_position_independence_proof: false,
      separate_range_attenuation_family_names_are_attack_position_independence_proof: false,
      relative_spatial_relation_tolerances_are_promotion_rules: false,
      relative_spatial_relation_equality_proves_all_spatial_damage_inputs_equal: false,
      packet_source_mismatches_are_preserved_as_unresolved: true,
      future_capture_capability_is_historical_observation_proof: false,
      enclosing_aoi_entity_is_provider_without_separate_proof: false,
      unresolved_evidence_hidden: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    operator_frontier: {
      current_client_contains_server_operator_implementation: false,
      attack_range_attenuation_family_names: attenuationFamilies.map((family) => ({
        damage_script: family.damage_script,
        candidate_rows: family.candidate_rows,
        operator_proof_state: family.operator_proof_state,
      })),
      conclusion: "family names classify exact-build static candidates but do not prove the server semantics of ordinary Attack or permit position exclusion",
    },
    relative_spatial_frontier: {
      relations: spatialRelations,
      direct_source_position_to_target_position: directSpatialRelation,
      direct_relation_complete_transition_pairs: directSpatialRelation.both_relations_complete_count,
      direct_relation_exact_displacement_transition_pairs:
        directSpatialRelation.exact_displacement_vector_equal_count,
      direct_relation_nonexact_displacement_transition_pairs:
        Number(directSpatialRelation.both_relations_complete_count) -
        Number(directSpatialRelation.exact_displacement_vector_equal_count),
      direct_relation_within_one_raw_coordinate_unit_transition_pairs:
        directSpatialRelation.absolute_distance_delta_le_1_count,
      direct_relation_outside_one_raw_coordinate_unit_transition_pairs:
        Number(directSpatialRelation.both_relations_complete_count) -
        Number(directSpatialRelation.absolute_distance_delta_le_1_count),
      spatial_state_safe_to_exclude_from_counterfactual_matching: false,
      formula_authority: false,
      conclusion: "most configured transitions change observed source-to-target geometry; absolute and relative spatial state remains required",
    },
    fake_bullet_lifecycle_frontier: {
      exact_wire_contract: fakeBulletLifecycle.exact_wire_contract,
      canonical_timeline_contract: fakeBulletLifecycle.canonical_timeline_contract,
      observed_evidence_frontier: fakeBulletLifecycle.observed_evidence_frontier,
      conclusion:
        "future captures can retain the exact source-4 lifecycle join keys, but no current-build observed lifecycle row or provider ownership proof exists yet",
    },
    summary: {
      component_action_identities: [...grouped.values()].reduce((sum, group) => sum + group.component_identities.length, 0),
      exact_action_selectors: actions.length,
      observed_transition_pairs: actions.reduce((sum, action) => sum + action.observed_transition_pairs, 0),
      packet_source_route_matched_selectors: actions.filter((action) => action.static_route.packet_source_matches_candidate_route).length,
      packet_source_route_matched_transition_pairs: matchingPairs,
      packet_source_route_rejected_selectors: actions.filter((action) => !action.static_route.packet_source_matches_candidate_route).length,
      packet_source_route_rejected_transition_pairs: rejectedPairs,
      rejected_selectors: actions.filter((action) => !action.static_route.packet_source_matches_candidate_route).map((action) => action.selector),
      exact_build_static_formula_candidates: actions.filter((action) => action.formula_candidate.selected_for_counterfactual_evaluation).length,
      direct_spatial_relation_complete_transition_pairs:
        directSpatialRelation.both_relations_complete_count,
      direct_spatial_relation_exact_transition_pairs:
        directSpatialRelation.exact_displacement_vector_equal_count,
      direct_spatial_relation_nonexact_transition_pairs:
        Number(directSpatialRelation.both_relations_complete_count) -
        Number(directSpatialRelation.exact_displacement_vector_equal_count),
      direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs:
        Number(directSpatialRelation.both_relations_complete_count) -
        Number(directSpatialRelation.absolute_distance_delta_le_1_count),
      spatial_state_safe_to_exclude_from_counterfactual_matching: false,
      fake_bullet_exact_wire_fields: 7,
      fake_bullet_future_capture_join_keys_retained: true,
      fake_bullet_current_build_observed_lifecycle_records: 0,
      fake_bullet_source4_damage_route_resolved: false,
      fake_bullet_provider_ownership_proven: false,
      strict_controlled_counterfactual_pairs: 0,
      formula_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    actions,
    next_action: "capture exact-build FakeBullets beside source-4 damage and prove the container-to-provider relation for 220101; retain AttrPos, AttrTargetPos, and every residual state dimension for the other 68 packet-source-compatible transitions before integer formula evaluation",
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  verify(report);
  return report;
}

function verify(report) {
  const summary = report.summary ?? {};
  if (
    report.schema_version !== SCHEMA_VERSION || report.generated_by !== GENERATED_BY || report.game_build !== GAME_BUILD ||
    Number(report.effect_id) !== EFFECT_ID || report.damage_relationship !== "source" ||
    report.source_identity?.exact_build_table_manifest_bindings_complete !== true ||
    report.policy?.structurally_unobservable_remote_player_cast_packets_are_required !== false ||
    report.policy?.static_table_values_are_server_operator_proof !== false ||
    report.policy?.static_zero_or_empty_spatial_fields_are_position_independence_proof !== false ||
    report.policy?.separate_range_attenuation_family_names_are_attack_position_independence_proof !== false ||
    report.policy?.relative_spatial_relation_tolerances_are_promotion_rules !== false ||
    report.policy?.relative_spatial_relation_equality_proves_all_spatial_damage_inputs_equal !== false ||
    report.policy?.packet_source_mismatches_are_preserved_as_unresolved !== true ||
    report.policy?.future_capture_capability_is_historical_observation_proof !== false ||
    report.policy?.enclosing_aoi_entity_is_provider_without_separate_proof !== false ||
    report.policy?.unresolved_evidence_hidden !== false || report.policy?.provider_rdps_credit_allowed !== false ||
    report.operator_frontier?.current_client_contains_server_operator_implementation !== false ||
    Number(summary.component_action_identities) !== 36 || Number(summary.exact_action_selectors) !== 7 ||
    Number(summary.observed_transition_pairs) !== 69 || Number(summary.packet_source_route_matched_selectors) !== 6 ||
    Number(summary.packet_source_route_matched_transition_pairs) !== 68 || Number(summary.packet_source_route_rejected_selectors) !== 1 ||
    Number(summary.packet_source_route_rejected_transition_pairs) !== 1 || Number(summary.exact_build_static_formula_candidates) !== 6 ||
    Number(summary.direct_spatial_relation_complete_transition_pairs) !== 66 ||
    Number(summary.direct_spatial_relation_exact_transition_pairs) !== 2 ||
    Number(summary.direct_spatial_relation_nonexact_transition_pairs) !== 64 ||
    Number(summary.direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12 ||
    summary.spatial_state_safe_to_exclude_from_counterfactual_matching !== false ||
    Number(summary.fake_bullet_exact_wire_fields) !== 7 ||
    summary.fake_bullet_future_capture_join_keys_retained !== true ||
    Number(summary.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
    summary.fake_bullet_source4_damage_route_resolved !== false ||
    summary.fake_bullet_provider_ownership_proven !== false ||
    Number(summary.strict_controlled_counterfactual_pairs) !== 0 || summary.formula_authority !== false ||
    summary.ui_display_authority !== false || summary.provider_rdps_credit_allowed !== false ||
    report.actions?.length !== 7 ||
    report.actions.some((action) => action.proof_state?.spatial_state_safe_to_exclude_from_counterfactual_matching !== false || action.proof_state?.provider_rdps_credit_allowed !== false) ||
    report.actions.filter((action) => action.static_route?.state === "rejected-packet-source-mismatch").length !== 1 ||
    report.actions.find((action) => action.packet_identity?.ability_id === 220101)?.packet_identity?.damage_source_effective_enum_name !== "EDamageSourceFakeBullet" ||
    report.actions.find((action) => action.packet_identity?.ability_id === 220101)?.unresolved_lifecycle?.exact_wire_join_keys_preserved_for_future_captures !== true ||
    Number(report.actions.find((action) => action.packet_identity?.ability_id === 220101)?.unresolved_lifecycle?.current_build_observed_lifecycle_records) !== 0 ||
    report.actions.find((action) => action.packet_identity?.ability_id === 220101)?.unresolved_lifecycle?.enclosing_aoi_entity_is_provider_proven !== false ||
    report.actions.find((action) => action.packet_identity?.ability_id === 220101)?.unresolved_lifecycle?.provider_rdps_credit_allowed !== false ||
    report.fake_bullet_lifecycle_frontier?.canonical_timeline_contract?.event_kind !== "unresolved_action" ||
    report.fake_bullet_lifecycle_frontier?.observed_evidence_frontier?.source4_to_damage_component_join_proven !== false ||
    report.relative_spatial_frontier?.relations == null ||
    Object.keys(report.relative_spatial_frontier.relations).length !== 6 ||
    Number(report.relative_spatial_frontier?.direct_relation_complete_transition_pairs) !== 66 ||
    Number(report.relative_spatial_frontier?.direct_relation_exact_displacement_transition_pairs) !== 2 ||
    Number(report.relative_spatial_frontier?.direct_relation_nonexact_displacement_transition_pairs) !== 64 ||
    Number(report.relative_spatial_frontier?.direct_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12 ||
    report.relative_spatial_frontier?.spatial_state_safe_to_exclude_from_counterfactual_matching !== false ||
    report.relative_spatial_frontier?.formula_authority !== false ||
    report.content_sha256 !== digest(report)
  ) fail("Fatal Spiral action/spatial frontier is unsafe or has an invalid digest");
}

function parse(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value == null) fail(`Invalid argument ${flag ?? "<missing>"}`);
    args[flag.slice(2)] = value;
  }
  return args;
}

function required(args, name) {
  if (!args[name]) fail(`Missing --${name}`);
  return args[name];
}

function selfTest() {
  const sample = {
    schema_version: SCHEMA_VERSION, generated_by: GENERATED_BY, game_build: GAME_BUILD, effect_id: EFFECT_ID,
    damage_relationship: "source",
    source_identity: { exact_build_table_manifest_bindings_complete: true },
    policy: {
      structurally_unobservable_remote_player_cast_packets_are_required: false,
      static_table_values_are_server_operator_proof: false,
      static_zero_or_empty_spatial_fields_are_position_independence_proof: false,
      separate_range_attenuation_family_names_are_attack_position_independence_proof: false,
      relative_spatial_relation_tolerances_are_promotion_rules: false,
      relative_spatial_relation_equality_proves_all_spatial_damage_inputs_equal: false,
      packet_source_mismatches_are_preserved_as_unresolved: true,
      future_capture_capability_is_historical_observation_proof: false,
      enclosing_aoi_entity_is_provider_without_separate_proof: false,
      unresolved_evidence_hidden: false, provider_rdps_credit_allowed: false,
    },
    operator_frontier: { current_client_contains_server_operator_implementation: false },
    relative_spatial_frontier: {
      relations: Object.fromEntries(Array.from({ length: 6 }, (_, index) => [`relation_${index}`, {}])),
      direct_relation_complete_transition_pairs: 66,
      direct_relation_exact_displacement_transition_pairs: 2,
      direct_relation_nonexact_displacement_transition_pairs: 64,
      direct_relation_outside_one_raw_coordinate_unit_transition_pairs: 12,
      spatial_state_safe_to_exclude_from_counterfactual_matching: false,
      formula_authority: false,
    },
    fake_bullet_lifecycle_frontier: {
      canonical_timeline_contract: { event_kind: "unresolved_action" },
      observed_evidence_frontier: { source4_to_damage_component_join_proven: false },
    },
    summary: {
      component_action_identities: 36, exact_action_selectors: 7, observed_transition_pairs: 69,
      packet_source_route_matched_selectors: 6, packet_source_route_matched_transition_pairs: 68,
      packet_source_route_rejected_selectors: 1, packet_source_route_rejected_transition_pairs: 1,
      exact_build_static_formula_candidates: 6, spatial_state_safe_to_exclude_from_counterfactual_matching: false,
      direct_spatial_relation_complete_transition_pairs: 66,
      direct_spatial_relation_exact_transition_pairs: 2,
      direct_spatial_relation_nonexact_transition_pairs: 64,
      direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs: 12,
      fake_bullet_exact_wire_fields: 7,
      fake_bullet_future_capture_join_keys_retained: true,
      fake_bullet_current_build_observed_lifecycle_records: 0,
      fake_bullet_source4_damage_route_resolved: false,
      fake_bullet_provider_ownership_proven: false,
      strict_controlled_counterfactual_pairs: 0, formula_authority: false, ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    actions: Array.from({ length: 7 }, (_, index) => ({
      packet_identity: { ability_id: index === 0 ? 220101 : index, damage_source_effective_enum_name: index === 0 ? "EDamageSourceFakeBullet" : "EDamageSourceBuff" },
      static_route: { state: index === 0 ? "rejected-packet-source-mismatch" : "exact-current-build-packet-source-match" },
      proof_state: { spatial_state_safe_to_exclude_from_counterfactual_matching: false, provider_rdps_credit_allowed: false },
      ...(index === 0 ? { unresolved_lifecycle: {
        exact_wire_join_keys_preserved_for_future_captures: true,
        current_build_observed_lifecycle_records: 0,
        enclosing_aoi_entity_is_provider_proven: false,
        provider_rdps_credit_allowed: false,
      } } : {}),
    })),
    content_sha256: "",
  };
  sample.content_sha256 = digest(sample);
  verify(sample);
  sample.summary.provider_rdps_credit_allowed = true;
  try {
    verify(sample);
    fail("self-test accepted provider credit");
  } catch (error) {
    if (error.message === "self-test accepted provider credit") throw error;
  }
  console.log("bpsr-fatal-spiral-transition-action-spatial-frontier self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(path.resolve(required(args, "input")), "action/spatial frontier"));
    console.log("Fatal Spiral action/spatial frontier verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = path.resolve(required(args, "output"));
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = build({
      transitionAudit: required(args, "transition-audit"), buildManifest: required(args, "build-manifest"),
      damageLedger: required(args, "damage-ledger"), damageScriptInventory: required(args, "damage-script-inventory"),
      combatSurface: required(args, "combat-surface"), skillTable: required(args, "skill-table"),
      fakeBulletLifecycle: required(args, "fake-bullet-lifecycle"),
      skillEffectTable: required(args, "skill-effect-table"), bulletTable: required(args, "bullet-table"),
      buffTable: required(args, "buff-table"), damageAttrTable: required(args, "damage-attr-table"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, summary: report.summary, content_sha256: report.content_sha256 }, null, 2));
  } else {
    console.log("Usage:\n  node tools/bpsr-fatal-spiral-transition-action-spatial-frontier.mjs build --transition-audit <json> --build-manifest <json> --damage-ledger <json> --damage-script-inventory <json> --combat-surface <json> --fake-bullet-lifecycle <json> --skill-table <json> --skill-effect-table <json> --bullet-table <json> --buff-table <json> --damage-attr-table <json> --output <json>\n  node tools/bpsr-fatal-spiral-transition-action-spatial-frontier.mjs verify --input <json>\n  node tools/bpsr-fatal-spiral-transition-action-spatial-frontier.mjs self-test");
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
