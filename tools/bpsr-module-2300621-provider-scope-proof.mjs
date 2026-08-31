#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import path from "node:path";

const SCHEMA_VERSION = 2;
const GENERATOR = "tools/bpsr-module-2300621-provider-scope-proof.mjs";
const BUILD = "24687926";
const ROOT_EFFECT_ID = 2_300_620;
const STACK_EFFECT_ID = 2_300_621;
const PROVIDER_ENTITY_UUID = 190_072_160_896;
const POWER_OF_UNITY_EFFECT_ID = 683_115;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") await build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function build(parsed) {
  const extractedTableDirectory = resolvedDirectory(parsed, "table-directory");
  const files = {
    events: resolved(parsed, "events"),
    cohort: resolved(parsed, "cohort"),
    counterfactual: resolved(parsed, "counterfactual"),
    module_raw_165: resolved(parsed, "module-raw-165"),
    module_raw_275: resolved(parsed, "module-raw-275"),
    mod_effect_table: resolved(parsed, "mod-effect-table"),
    affix_table: resolved(parsed, "affix-table"),
    buff_table: resolved(parsed, "buff-table"),
    affix_table_24568685: resolved(parsed, "affix-table-24568685"),
    buff_table_24568685: resolved(parsed, "buff-table-24568685"),
    affix_table_24609362: resolved(parsed, "affix-table-24609362"),
    buff_table_24609362: resolved(parsed, "buff-table-24609362"),
    damage_attr_table: resolved(parsed, "damage-attr-table"),
    damage_stage_runtime: resolved(parsed, "damage-stage-runtime"),
    damage_attr_formula_stage: resolved(parsed, "damage-attr-formula-stage"),
    formula_runtime: resolved(parsed, "formula-runtime"),
    value_runtime: resolved(parsed, "value-runtime"),
    community_modules_calc: resolved(parsed, "community-modules-calc"),
    community_damage_calc: resolved(parsed, "community-damage-calc"),
  };
  const output = path.resolve(required(parsed, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);

  const eventAudit = await scanEvents(files.events);
  const documents = {
    cohort: readJson(files.cohort, "cohort"),
    counterfactual: readJson(files.counterfactual, "counterfactual"),
    module_raw_165: readJson(files.module_raw_165, "module raw 165"),
    module_raw_275: readJson(files.module_raw_275, "module raw 275"),
    mod_effect_table: readJson(files.mod_effect_table, "ModEffectTable"),
    affix_table: readJson(files.affix_table, "AffixTable"),
    buff_table: readJson(files.buff_table, "BuffTable"),
    affix_table_24568685: readJson(files.affix_table_24568685, "build 24568685 AffixTable"),
    buff_table_24568685: readJson(files.buff_table_24568685, "build 24568685 BuffTable"),
    affix_table_24609362: readJson(files.affix_table_24609362, "build 24609362 AffixTable"),
    buff_table_24609362: readJson(files.buff_table_24609362, "build 24609362 BuffTable"),
    damage_attr_table: readJson(files.damage_attr_table, "DamageAttrTable"),
    damage_stage_runtime: readJson(files.damage_stage_runtime, "damage-stage runtime"),
    damage_attr_formula_stage: readJson(files.damage_attr_formula_stage, "DamageAttr formula-stage runtime"),
    formula_runtime: readJson(files.formula_runtime, "formula runtime"),
    value_runtime: readJson(files.value_runtime, "value runtime"),
    community_modules_calc: readFileSync(files.community_modules_calc, "utf8"),
    community_damage_calc: readFileSync(files.community_damage_calc, "utf8"),
  };
  const inputs = {};
  for (const [key, file] of Object.entries(files)) inputs[key] = await descriptor(file);
  inputs.extracted_table_census = scanExactIdTableDirectory(
    extractedTableDirectory, POWER_OF_UNITY_EFFECT_ID);
  const report = buildReport(documents, eventAudit, inputs);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote ${output}`);
}

function buildReport(documents, eventAudit, inputs) {
  const cohort = documents.cohort;
  assert(cohort?.schema_version === 46 && String(cohort?.game_build) === BUILD,
    "Formula cohort identity mismatch");
  const selectedSamples = cohort.samples.filter((sample) =>
    sample.source_entity_uuid === PROVIDER_ENTITY_UUID && sample.ability_id === 2_031_101 &&
    ((sample.target_entity_uuid === 7_086_473_280 && [74_013, 75_636].includes(sample.amount)) ||
     (sample.target_entity_uuid === 7_086_604_352 && [39_963, 40_840].includes(sample.amount))));
  assert(selectedSamples.length === 4, "Expected four exact provider-owned ladder samples");
  assert(unique(selectedSamples.map((sample) => sample.source_attribute_state_id)).length === 1 &&
    unique(selectedSamples.map((sample) => sample.source_status_state_id)).length === 1,
  "Provider source state changed inside the selected ladders");
  const sourceStatusStateId = selectedSamples[0].source_status_state_id;
  const sourceStatuses = cohort.status_states[sourceStatusStateId] ?? [];
  const unity = sourceStatuses.filter((status) => status.effect_id === POWER_OF_UNITY_EFFECT_ID);
  const root = sourceStatuses.filter((status) => status.effect_id === ROOT_EFFECT_ID);
  assert(unity.length === 1 && unity[0].source_entity_uuid === PROVIDER_ENTITY_UUID,
    "Power of Unity was not provider-owned in the selected ladders");
  assert(root.length === 1 && root[0].source_entity_uuid === PROVIDER_ENTITY_UUID,
    "DMG Stack root was not provider-owned in the selected ladders");

  const rawProofs = [documents.module_raw_165, documents.module_raw_275];
  for (const proof of rawProofs) {
    assert(proof?.schema_version === 1 && proof?.effect_id === STACK_EFFECT_ID &&
      [165, 275].includes(proof.configured_raw_value_per_stack),
    "Module ladder proof identity mismatch");
    assert(/not per-hit causal order/.test(proof.policy?.ordering_scope ?? ""),
      "Module ladder proof no longer preserves its same-wire causal limitation");
  }
  const selectedLadderModels = Object.fromEntries(rawProofs.map((proof) => {
    const ladders = proof.ladders.filter((ladder) =>
      ladder.capture_sequence === 6_921 &&
      ladder.identity.source_entity_uuid === PROVIDER_ENTITY_UUID &&
      [7_086_473_280, 7_086_604_352].includes(ladder.identity.target_entity_uuid));
    assert(ladders.length === 2, `Missing capture 6921 ladders for raw ${proof.configured_raw_value_per_stack}`);
    return [String(proof.configured_raw_value_per_stack),
      ladders.map((ladder) => compactLadder(ladder, cohort))];
  }));
  const raw165Zones = selectedLadderModels["165"].flatMap((row) => row.baseline_zone_raw_candidates);
  const raw275Zones = selectedLadderModels["275"].flatMap((row) => row.baseline_zone_raw_candidates);
  const raw165Intersection = intersectRanges(selectedLadderModels["165"]
    .map((row) => range(row.baseline_zone_raw_candidates)));
  const raw275Intersection = intersectRanges(selectedLadderModels["275"]
    .map((row) => range(row.baseline_zone_raw_candidates)));
  assert(Math.max(...raw165Zones) < 0 && Math.min(...raw275Zones) >= 2_500 &&
    Math.max(...raw275Zones) <= 2_550,
  "The scalar discriminator ranges changed");
  assert(raw165Intersection.minimum === -2_479 && raw165Intersection.maximum === -2_475 &&
    raw275Intersection.minimum === 2_536 && raw275Intersection.maximum === 2_544,
  "The cross-stratum common baseline intersections changed");
  assert(selectedLadderModels["275"].every((row) =>
    row.wire_start_status_state_is_constant === true &&
    row.skill_effect_component_indices.length === 2 &&
    row.skill_effect_component_indices[0] === 0),
  "Selected same-wire component evidence changed");

  const formulaUnity = documents.formula_runtime.entriesByKey?.[`buffs:${POWER_OF_UNITY_EFFECT_ID}`];
  const valueUnity = documents.value_runtime.entriesByKey?.[`buffs:${POWER_OF_UNITY_EFFECT_ID}`];
  assert(formulaUnity?.formulaReadiness === "description-grounded-needs-runtime-proof" &&
    formulaUnity.formulaZoneIds.includes("generalDamage") &&
    formulaUnity.formulaZoneIds.includes("skillMultiplier"),
  "Power of Unity formula frontier changed");
  const unityValue = valueUnity?.selectedValues?.find((row) =>
    row.componentKey === "generic-damage" && row.value === 25);
  assert(unityValue, "Power of Unity exact description value is missing");
  assert(/moduleType === ['"]damage-stack['"][\s\S]{0,800}?dmgBonuses\s*=\s*\[0,\s*0,\s*0,\s*0,\s*0,\s*6\.6,\s*11\][\s\S]{0,200}?moduleAllDmgBonus/.test(
    documents.community_modules_calc), "Pinned community module formula changed");
  assert(/genDmgPct\s*=\s*[\s\S]{0,500}?moduleAllDmgBonus\s*\/\s*100/.test(
    documents.community_damage_calc), "Pinned community generic-damage aggregation changed");

  const moduleRows = Object.values(documents.mod_effect_table)
    .filter((row) => Number(row.EffectID) === 2_104)
    .sort((left, right) => Number(left.Level) - Number(right.Level));
  const legalRows = moduleRows.filter((row) => [5, 6].includes(Number(row.Level)));
  assert(legalRows.length === 2, "Module 2104 level 5/6 rows are missing");
  const moduleLevelCandidates = legalRows.map((row) => ({
    level: Number(row.Level),
    row_id: Number(row.Id),
    total_link_points: Number(row.EnhancementNum),
    effect_config: row.EffectConfig,
    effect_value: row.EffectValue,
    stack_raw_value: Number(row.EffectValue?.[0]?.[1]),
  }));
  assertExact(moduleLevelCandidates.map((row) => row.stack_raw_value), [165, 275],
    "Module scalar candidates");

  const affixRows = Object.values(documents.affix_table);
  const opcode14Rows = affixRows.filter((row) => (row.Effect ?? []).some((effect) =>
    Number(effect?.[0]) === 1 && Number(effect?.[1]) === 4));
  const opcode14ThirdFieldZero = opcode14Rows.filter((row) =>
    Number(row.Effect.find((effect) => Number(effect?.[0]) === 1 && Number(effect?.[1]) === 4)?.[2]) === 0);
  const opcode14ThirdFieldNonzero = opcode14Rows.filter((row) =>
    Number(row.Effect.find((effect) => Number(effect?.[0]) === 1 && Number(effect?.[1]) === 4)?.[2]) !== 0);
  const powerOfUnityAffixes = opcode14Rows.filter((row) => [119, 400].includes(Number(row.Id)))
    .map((row) => ({
      id: Number(row.Id),
      effect: row.Effect,
      effect_type: Number(row.EffectType),
      target_type: Number(row.TargetType),
      semantic_description: row.Description,
    }));
  assert(affixRows.length === 257 && opcode14Rows.length === 179 &&
    opcode14ThirdFieldZero.length === 101 && opcode14ThirdFieldNonzero.length === 78,
  "Affix opcode census changed");
  assert(powerOfUnityAffixes.length === 2 && powerOfUnityAffixes.every((row) =>
    row.effect.some((effect) => Number(effect?.[0]) === 1 && Number(effect?.[1]) === 4 &&
      Number(effect?.[2]) === 0 && Number(effect?.[3]) === 0)),
  "Power of Unity affix rows changed");
  const powerOfUnityBuff = documents.buff_table[String(POWER_OF_UNITY_EFFECT_ID)];
  assert(Number(powerOfUnityBuff?.Id) === POWER_OF_UNITY_EFFECT_ID &&
    powerOfUnityBuff?.IsClientBuff === false && Number(powerOfUnityBuff?.SkillId) === 0 &&
    Array.isArray(powerOfUnityBuff?.SpecialAttr) && powerOfUnityBuff.SpecialAttr.length === 0,
  "Power of Unity server-controlled buff row changed");
  assert(inputs.affix_table.sha256 === inputs.affix_table_24568685.sha256 &&
    inputs.affix_table.sha256 === inputs.affix_table_24609362.sha256 &&
    inputs.buff_table.sha256 === inputs.buff_table_24568685.sha256 &&
    inputs.buff_table.sha256 === inputs.buff_table_24609362.sha256,
  "Season-3 AffixTable or BuffTable identity changed");
  assertExact(inputs.extracted_table_census.matching_files, ["BuffTable.json"],
    "Power of Unity extracted-table exact-ID surface");

  const counterfactual = documents.counterfactual;
  assert(counterfactual?.schema_version === 20 && String(counterfactual?.game_build) === BUILD,
    "Counterfactual identity mismatch");
  const counterfactualVariants = counterfactual.effects
    .filter((effect) => effect.effect_id === STACK_EFFECT_ID && effect.locus === "target")
    .flatMap((effect) => effect.variants);
  const externalExamples = counterfactualVariants
    .flatMap((variant) => variant.target_current_hp_excluded_diagnostic?.divergent_examples ?? [])
    .filter((example) => example.provider_relationship === "third_party" &&
      example.status?.source_entity_uuid === PROVIDER_ENTITY_UUID);
  assert(externalExamples.length === 1, "Expected one independent full-run third-party transition");
  const external = externalExamples[0];
  assert(external.source_entity_uuid === 216_009_015_936 && external.ability_id === 55_240 &&
    external.status.stacks === 3 && external.present_outcome.amount === 99_541 &&
    external.absent_outcome.amount === 98_035,
  "Independent third-party transition changed");
  const nondeterministicExamples = counterfactualVariants
    .flatMap((variant) => variant.target_current_hp_excluded_diagnostic?.nondeterministic_examples ?? [])
    .filter((example) => example.status?.source_entity_uuid === PROVIDER_ENTITY_UUID);
  assert(nondeterministicExamples.length === 1,
    "Expected one bounded nondeterministic comparison");
  const nondeterministic = nondeterministicExamples[0];
  const nondeterministicOverlap = nondeterministic.present_outcomes.filter((present) =>
    nondeterministic.absent_outcomes.some((absent) =>
      stableStringify(absent.outcome) === stableStringify(present.outcome)));
  assert(nondeterministic.provider_relationship === "credited_damage_source" &&
    nondeterministic.present_sample_count === 1 && nondeterministic.absent_sample_count === 2 &&
    nondeterministicOverlap.length === 1 &&
    nondeterministicOverlap[0].outcome.amount === 78_375,
  "Nondeterministic same-source overlap changed");
  const selectedDamageStageRules = documents.damage_stage_runtime.rules.filter((row) =>
    Number(row.ability_id) === 55_240 && Number(row.hit_event_id) === 3);
  assert(selectedDamageStageRules.length === 1 &&
    Number(selectedDamageStageRules[0].damage_attr_id) === 25_524_003 &&
    selectedDamageStageRules[0].damage_script === "Attack",
  "Selected independent transition damage-stage mapping changed");
  const selectedDamageAttr = documents.damage_attr_table["25524003"];
  assert(Number(selectedDamageAttr?.Id) === 25_524_003 &&
    selectedDamageAttr?.DamageScript === "Attack" &&
    Object.keys(selectedDamageAttr).every((key) => !/hp|health/i.test(key)),
  "Selected independent transition DamageAttr row changed");
  const damageAttrRows = Object.values(documents.damage_attr_table);
  const damageScriptCounts = Object.fromEntries([...new Set(damageAttrRows
    .map((row) => String(row.DamageScript ?? "")))].sort().map((script) => [
    script || "<empty>",
    damageAttrRows.filter((row) => String(row.DamageScript ?? "") === script).length,
  ]));
  assert(damageAttrRows.length === 5_700 && damageScriptCounts.Attack === 3_111 &&
    damageScriptCounts.TargetHpHeal === 34 && damageScriptCounts.AddShieldByHp === 22 &&
    damageScriptCounts.HealByHp === 4,
  "DamageScript census changed");
  assert(documents.damage_attr_formula_stage?.calculation_modes?.standard_attack?.
    client_ui_expression === "physical_or_magical_attack * PVEDamageRadio + PVEFixedParameter" &&
    documents.damage_attr_formula_stage?.policy?.runtime_formula_authority === false,
  "Standard Attack formula-stage boundary changed");
  const presentPacket = external.present_formula_context.normalized_packet_inputs;
  const absentPacket = external.absent_formula_context.normalized_packet_inputs;
  assert(external.present_outcome.actual_amount === null &&
    external.absent_outcome.actual_amount === null &&
    presentPacket.skill_effect_total_damage === null &&
    absentPacket.skill_effect_total_damage === null &&
    presentPacket.owner_stage === null && absentPacket.owner_stage === null &&
    Object.values(presentPacket.damage_weight).every((value) => value === null) &&
    Object.values(absentPacket.damage_weight).every((value) => value === null),
  "Selected transition unexpectedly acquired a server roll or subtotal field");

  assert(eventAudit.actor_events > 0 && eventAudit.root_status_events > 0 &&
    eventAudit.stack_status_events > 0 && eventAudit.module_level_fields_observed === false,
  "Canonical provider timeline audit mismatch");

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: BUILD,
    root_effect_id: ROOT_EFFECT_ID,
    stack_effect_id: STACK_EFFECT_ID,
    provider_entity_uuid: PROVIDER_ENTITY_UUID,
    inputs,
    policy: {
      exact_numeric_ids_and_build_identity_authoritative: true,
      localized_names_are_semantic_evidence_only: true,
      remote_module_level_is_not_invented: true,
      target_current_hp_is_not_silently_ignored: true,
      unresolved_formula_zone_is_preserved: true,
      provider_rdps_credit_fail_closed: true,
    },
    canonical_provider_timeline: eventAudit,
    provider_owned_ladder_context: {
      source_attribute_state_id: selectedSamples[0].source_attribute_state_id,
      source_status_state_id: sourceStatusStateId,
      source_statuses: { power_of_unity_683115: unity[0], dmg_stack_root_2300620: root[0] },
      ladders: groupSamples(selectedSamples),
    },
    exact_current_build_module_candidates: moduleLevelCandidates,
    affix_opcode_1_4_census: {
      total_affix_rows: affixRows.length,
      matching_rows: opcode14Rows.length,
      third_field_zero_rows: opcode14ThirdFieldZero.length,
      third_field_nonzero_rows: opcode14ThirdFieldNonzero.length,
      power_of_unity_rows: powerOfUnityAffixes,
      representative_non_damage_semantics: opcode14Rows
        .filter((row) => [15, 19, 20, 21, 39, 5001].includes(Number(row.Id)))
        .map((row) => ({ id: Number(row.Id), semantic_description: row.Description })),
      localized_descriptions_are_semantic_evidence_only: true,
      opcode_uniquely_identifies_general_damage_formula_zone: false,
      conclusion: "The [1,4] pair is a broad affix dispatch shape, not a unique general-damage formula-zone key; Power of Unity carries no scalar or formula-stage payload in this row.",
    },
    power_of_unity_current_build_table_boundary: {
      exact_effect_id: POWER_OF_UNITY_EFFECT_ID,
      buff_row: powerOfUnityBuff,
      exact_id_found_only_in_buff_table_by_bounded_extracted_table_census: true,
      client_owned_effect: false,
      formula_payload_exposed_by_client_tables: false,
      conclusion: "The exact status is server-controlled and the retained client tables expose its magnitude text but not its damage-operation bucket or integer boundary.",
    },
    same_season_table_stability: {
      builds: ["24568685", "24609362", BUILD],
      affix_table_sha256: inputs.affix_table.sha256,
      buff_table_sha256: inputs.buff_table.sha256,
      whole_files_byte_identical: true,
      implication: "The stable client evidence migrates identity and semantic behavior across these Season-3 builds, but cannot create missing server formula authority.",
    },
    scalar_discriminator: {
      raw_165: {
        ladders: selectedLadderModels["165"],
        baseline_zone_raw_range: range(raw165Zones),
        cross_critical_and_noncritical_common_baseline_zone_raw: raw165Intersection,
        conflicts_with_additive_power_of_unity_2500_as_the_only_baseline_term: true,
      },
      raw_275: {
        ladders: selectedLadderModels["275"],
        baseline_zone_raw_range: range(raw275Zones),
        cross_critical_and_noncritical_common_baseline_zone_raw: raw275Intersection,
        matches_additive_power_of_unity_2500_with_integer_preimage_width: true,
        same_wire_different_component_fit_only: true,
        causal_stack_transition_proven: false,
      },
      power_of_unity_static_value_raw: 2_500,
      power_of_unity_formula_zone_candidates: formulaUnity.formulaZoneIds,
      exact_provider_scalar_if_power_of_unity_is_additive_general_damage: 275,
      power_of_unity_formula_zone_uniquely_proven: false,
      exact_provider_scalar_unconditionally_proven: false,
      same_wire_decoder_order_is_server_causal_order: false,
    },
    community_formula_corroboration: {
      repository_commit: "e21e06c07559396d4432c2541319c7c08e5caf31",
      damage_stack_level_5_total_generic_damage_percent_at_four_stacks: 6.6,
      damage_stack_level_6_total_generic_damage_percent_at_four_stacks: 11,
      implied_per_stack_raw_values: [165, 275],
      module_damage_is_added_to_generic_damage_bucket: true,
      evidence_role: "non-authoritative hypothesis corroboration only; exact game tables and packet damage remain promotion authority",
    },
    external_target_wide_transition: compactCounterfactual(external),
    same_source_nondeterministic_roll_overlap: {
      provider_relationship: nondeterministic.provider_relationship,
      provider_entity_uuid: nondeterministic.status.source_entity_uuid,
      damage_source_entity_uuid: nondeterministic.source_entity_uuid,
      ability_id: nondeterministic.ability_id,
      provider_stacks: nondeterministic.status.stacks,
      present_sample_count: nondeterministic.present_sample_count,
      absent_sample_count: nondeterministic.absent_sample_count,
      present_outcomes: nondeterministic.present_outcomes,
      absent_outcomes: nondeterministic.absent_outcomes,
      overlapping_outcomes: nondeterministicOverlap,
      implication: "The same output occurs with and without the status under the recorded comparison key; this is direct evidence that hidden server variation remains and that status presence alone cannot select the damage transform.",
      formula_authority: false,
      provider_rdps_credit_allowed: false,
    },
    external_transition_current_hp_and_server_roll_boundary: {
      ability_id: 55_240,
      hit_event_id: 3,
      damage_attr_id: 25_524_003,
      exact_damage_stage_rule: selectedDamageStageRules[0],
      exact_damage_attr_row: selectedDamageAttr,
      damage_script_census: damageScriptCounts,
      exact_target_attribute_difference_ids: attributeDifferenceIds(
        external.present_formula_context.target_attributes,
        external.absent_formula_context.target_attributes),
      selected_row_has_configured_hp_field: false,
      selected_script_is_hp_named: false,
      standard_attack_named_component_expression:
        documents.damage_attr_formula_stage.calculation_modes.standard_attack.client_ui_expression,
      configured_target_current_hp_formula_input_for_selected_row: false,
      packet_pre_mitigation_total_observed: false,
      packet_actual_amount_observed: false,
      packet_damage_weight_or_roll_observed: false,
      packet_owner_stage_observed: false,
      shared_server_roll_between_present_and_absent_samples_proven: false,
      hidden_server_input_or_roll_fully_excluded: false,
      conclusion: "The exact current client formula surface closes configured target-CurrentHP use for this standard Attack row, but the two packet results do not expose or prove one shared server roll; the 1506 output delta remains diagnostic rather than exact provider credit.",
    },
    adjudication: {
      provider_to_target_status_ownership_proven: true,
      target_status_controlled_groups_observed: 2,
      third_party_target_status_controlled_groups_observed: 1,
      same_damage_source_nondeterministic_groups_observed: 1,
      third_party_deterministic_divergent_output_groups_observed: 1,
      server_filtered_self_only_interpretation_refuted_by_observed_damage_transition: false,
      reason_not_refuted: "The exact configured Attack row excludes a target-CurrentHP input, but the packet does not expose or prove a shared server damage roll or exclude hidden server inputs.",
      provider_module_level_observed_in_packets: false,
      scalar_275_strongly_selected_by_same_wire_component_fit: true,
      scalar_275_causally_selected_by_current_build_damage: false,
      scalar_275_production_authority: false,
      affix_opcode_4_proves_power_of_unity_formula_zone: false,
      client_tables_prove_power_of_unity_formula_zone: false,
      power_of_unity_server_control_boundary_proven: true,
      selected_55240_row_configured_target_current_hp_input_excluded: true,
      selected_55240_pair_shared_server_roll_proven: false,
      operation_order_proven: false,
      integer_rounding_unique: false,
      formula_specific_conservation_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
    smallest_safe_next_slice: [
      "acquire a controlled Power of Unity 683115 absent/present damage isolation; the exact client tables prove a server-control boundary and expose no native formula handler",
      "acquire replicated fixed-context 55240 hit-3 samples before/during/after provider 190's exact stack lifecycle to solve the hidden server-roll and integer-boundary distribution",
      "replay a 275-per-stack target-wide counterfactual with simultaneous providers and exact integer conservation",
    ],
    promotion_counts: {
      production_runtime_ui: 0,
      superseded_historical_experiments: 2,
      active_current_build_candidates: 1,
    },
  };
}

async function scanEvents(file) {
  const stream = createReadStream(file);
  const lines = createInterface({ input: stream, crlfDelay: Infinity });
  let actorEvents = 0;
  let rootStatusEvents = 0;
  let stackStatusEvents = 0;
  const actorFields = new Set();
  const primaryLoadouts = new Map();
  const loadoutObservations = new Map();
  const statusTargets = new Set();
  const stackCounts = new Set();
  const statusStates = new Map();
  const durationMillis = new Set();
  for await (const line of lines) {
    if (!line.includes(String(PROVIDER_ENTITY_UUID))) continue;
    const envelope = JSON.parse(line);
    const kind = envelope.event?.type === "timeline" ? envelope.event.data?.kind : null;
    if (kind?.event === "actor" && kind.data?.actor?.entity_uuid === PROVIDER_ENTITY_UUID) {
      actorEvents += 1;
      for (const [key, value] of Object.entries(kind.data)) {
        if (value !== null && value !== undefined) actorFields.add(key);
      }
      if (kind.data.primary_loadout?.length) {
        primaryLoadouts.set(stableStringify(kind.data.primary_loadout), kind.data.primary_loadout);
      }
      if (kind.data.loadout_observation) {
        loadoutObservations.set(stableStringify(kind.data.loadout_observation),
          kind.data.loadout_observation);
      }
      continue;
    }
    if (kind?.event !== "status" || kind.data?.source?.entity_uuid !== PROVIDER_ENTITY_UUID) continue;
    if (kind.data.effect === ROOT_EFFECT_ID) rootStatusEvents += 1;
    else if (kind.data.effect === STACK_EFFECT_ID) stackStatusEvents += 1;
    else continue;
    statusTargets.add(kind.data.target?.entity_uuid);
    stackCounts.add(kind.data.stacks);
    statusStates.set(kind.data.state, (statusStates.get(kind.data.state) ?? 0) + 1);
    if (kind.data.duration_millis !== null) durationMillis.add(kind.data.duration_millis);
  }
  const prohibitedModuleFields = [...actorFields].filter((key) => /module|mod_effect|link_point/i.test(key));
  return {
    input_lines_scanned_streaming: true,
    actor_events: actorEvents,
    actor_fields_observed: [...actorFields].sort(),
    primary_loadouts_observed: [...primaryLoadouts.values()],
    loadout_observations: [...loadoutObservations.values()],
    module_level_fields_observed: prohibitedModuleFields.length > 0,
    module_level_field_names: prohibitedModuleFields,
    root_status_events: rootStatusEvents,
    stack_status_events: stackStatusEvents,
    unique_status_target_entity_uuids: statusTargets.size,
    observed_stack_counts: [...stackCounts].sort((a, b) => a - b),
    observed_lifecycle_states: Object.fromEntries([...statusStates].sort()),
    observed_non_null_duration_millis: [...durationMillis].sort((a, b) => a - b),
    exact_relationship_shape: "provider -> effect/status lifecycle -> recipient or enemy target; recipient damage action -> recipient/enemy target",
  };
}

function compactLadder(ladder, cohort) {
  const cohortSamples = ladder.ordered_damage_sequences.map((row) => {
    const sample = cohort.samples.find((candidate) =>
      candidate.sequence === row.envelope_sequence &&
      candidate.wire_capture_sequence === ladder.capture_sequence);
    assert(sample, `Missing cohort sample for ladder sequence ${row.envelope_sequence}`);
    return sample;
  });
  return {
    capture_sequence: ladder.capture_sequence,
    target_entity_uuid: ladder.identity.target_entity_uuid,
    critical: ladder.identity.critical,
    lucky: ladder.identity.lucky,
    stack_amounts: ladder.stack_amounts,
    skill_effect_group_indices: unique(cohortSamples.map((row) =>
      row.packet.skill_effect_group_index)),
    skill_effect_component_indices: cohortSamples.map((row) =>
      row.packet.skill_effect_component_index),
    wire_start_status_state_ids: unique(cohortSamples.map((row) =>
      row.target_status_state_id)),
    wire_start_status_state_is_constant: unique(cohortSamples.map((row) =>
      row.target_status_state_id)).length === 1,
    baseline_zone_raw_candidates: unique(ladder.formula_model_candidates
      .map((candidate) => candidate.baseline_zone_raw)),
    rounding_candidates: unique(ladder.formula_model_candidates
      .map((candidate) => candidate.rounding)),
  };
}

function groupSamples(samples) {
  return [...new Set(samples.map((sample) => sample.target_entity_uuid))].map((target) => {
    const rows = samples.filter((sample) => sample.target_entity_uuid === target)
      .sort((left, right) => left.amount - right.amount);
    return {
      target_entity_uuid: target,
      target_monster_id: rows[0].target_actor_identity?.monster_id ?? null,
      critical: rows[0].critical,
      lucky: rows[0].lucky,
      amounts: rows.map((row) => row.amount),
      envelope_sequences: rows.map((row) => row.sequence),
      wire_capture_sequences: unique(rows.map((row) => row.wire_capture_sequence)),
    };
  });
}

function compactCounterfactual(example) {
  const present = example.present_formula_context;
  const absent = example.absent_formula_context;
  return {
    provider_entity_uuid: example.status.source_entity_uuid,
    provider_stacks: example.status.stacks,
    recipient_damage_source_entity_uuid: example.source_entity_uuid,
    recipient_damage_action_id: example.ability_id,
    target_entity_uuid: example.target_entity_uuid,
    target_monster_id: attributeValue(present.target_attributes, 10),
    present_amount: example.present_outcome.amount,
    absent_amount: example.absent_outcome.amount,
    observed_delta: example.present_outcome.amount - example.absent_outcome.amount,
    present_sequences: example.present_sequences,
    absent_sequences: example.absent_sequences,
    source_attribute_state_equal: present.source_attribute_state_id === absent.source_attribute_state_id,
    source_status_state_equal: present.source_status_state_id === absent.source_status_state_id,
    target_attribute_difference_only_current_hp: attributeDifferenceIds(
      present.target_attributes, absent.target_attributes).every((id) => id === 11_310),
    target_status_difference_only_selected_provider_instance: selectedStatusDifferenceOnly(
      present.target_statuses, absent.target_statuses, example.status),
  };
}

function verifyCommand(parsed) {
  const input = resolved(parsed, "input");
  verifyReport(readJson(input, "input"));
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  assert(report?.schema_version === SCHEMA_VERSION && report?.generated_by === GENERATOR,
    "Report identity mismatch");
  assert(report?.game_build === BUILD && report?.root_effect_id === ROOT_EFFECT_ID &&
    report?.stack_effect_id === STACK_EFFECT_ID &&
    report?.provider_entity_uuid === PROVIDER_ENTITY_UUID, "Report exact identity mismatch");
  assert(report?.canonical_provider_timeline?.module_level_fields_observed === false &&
    report?.canonical_provider_timeline?.stack_status_events > 0,
  "Provider timeline proof mismatch");
  assert(report?.scalar_discriminator?.raw_165?.baseline_zone_raw_range?.maximum < 0 &&
    report?.scalar_discriminator?.raw_275?.baseline_zone_raw_range?.minimum >= 2_500 &&
    report?.scalar_discriminator?.exact_provider_scalar_unconditionally_proven === false &&
    report?.scalar_discriminator?.raw_275?.causal_stack_transition_proven === false &&
    report?.scalar_discriminator?.same_wire_decoder_order_is_server_causal_order === false,
  "Scalar discriminator mismatch");
  assert(report?.affix_opcode_1_4_census?.total_affix_rows === 257 &&
    report?.affix_opcode_1_4_census?.matching_rows === 179 &&
    report?.affix_opcode_1_4_census?.opcode_uniquely_identifies_general_damage_formula_zone === false,
  "Affix opcode census mismatch");
  assert(report?.power_of_unity_current_build_table_boundary?.exact_effect_id === POWER_OF_UNITY_EFFECT_ID &&
    report?.power_of_unity_current_build_table_boundary?.client_owned_effect === false &&
    report?.power_of_unity_current_build_table_boundary?.formula_payload_exposed_by_client_tables === false &&
    report?.same_season_table_stability?.whole_files_byte_identical === true &&
    JSON.stringify(report?.inputs?.extracted_table_census?.matching_files) ===
      JSON.stringify(["BuffTable.json"]),
  "Power of Unity client/server boundary mismatch");
  assert(report?.external_target_wide_transition?.provider_entity_uuid === PROVIDER_ENTITY_UUID &&
    report?.external_target_wide_transition?.recipient_damage_source_entity_uuid === 216_009_015_936,
  "External transition identity mismatch");
  assert(report?.same_source_nondeterministic_roll_overlap?.provider_relationship ===
    "credited_damage_source" &&
    report?.same_source_nondeterministic_roll_overlap?.overlapping_outcomes?.[0]?.outcome?.amount ===
      78_375 &&
    report?.same_source_nondeterministic_roll_overlap?.provider_rdps_credit_allowed === false,
  "Same-source nondeterministic overlap mismatch");
  assert(report?.external_transition_current_hp_and_server_roll_boundary?.damage_attr_id === 25_524_003 &&
    report?.external_transition_current_hp_and_server_roll_boundary?.
      configured_target_current_hp_formula_input_for_selected_row === false &&
    report?.external_transition_current_hp_and_server_roll_boundary?.
      shared_server_roll_between_present_and_absent_samples_proven === false,
  "External transition CurrentHP/server-roll boundary mismatch");
  assert(report?.adjudication?.provider_rdps_credit_allowed === false &&
    report?.adjudication?.runtime_promotion_allowed === false &&
    report?.promotion_counts?.production_runtime_ui === 0,
  "Report granted unsafe production authority");
  if (report.content_sha256 !== undefined) {
    assert(report.content_sha256 === contentHash(report), "Content hash mismatch");
  }
}

function selfTest() {
  assertExact(range([-2, 4, 1]), { minimum: -2, maximum: 4 }, "Range self-test");
  assert(selectedStatusDifferenceOnly(
    [{ effect_id: 7, source_entity_uuid: 9, stacks: 2, level: 1 }], [],
    { effect_id: 7, source_entity_uuid: 9, stacks: 2, level: 1 }),
  "Status difference self-test failed");
  assert([[1, 4, 0, 0]].some((effect) => Number(effect?.[0]) === 1 &&
    Number(effect?.[1]) === 4), "Affix opcode shape self-test failed");
  console.log("bpsr-module-2300621-provider-scope-proof self-test passed");
}

function selectedStatusDifferenceOnly(present, absent, selected) {
  const key = (row) => [row.effect_id, row.source_entity_uuid, row.stacks, row.level,
    row.origin_source_type_id ?? null, row.origin_source_config_id ?? null].join("|");
  const left = new Map(present.map((row) => [key(row), row]));
  const right = new Map(absent.map((row) => [key(row), row]));
  const onlyPresent = [...left].filter(([id]) => !right.has(id)).map(([, row]) => row);
  const onlyAbsent = [...right].filter(([id]) => !left.has(id)).map(([, row]) => row);
  return onlyPresent.length === 1 && onlyAbsent.length === 0 &&
    key(onlyPresent[0]) === key(selected);
}

function attributeDifferenceIds(left, right) {
  const a = new Map(left.map((row) => [row.attribute_id, row.value]));
  const b = new Map(right.map((row) => [row.attribute_id, row.value]));
  return unique([...a.keys(), ...b.keys()]).filter((id) => a.get(id) !== b.get(id));
}

function attributeValue(attributes, id) {
  return attributes.find((row) => row.attribute_id === id)?.value ?? null;
}

function range(values) {
  assert(values.length > 0, "Cannot take range of an empty list");
  return { minimum: Math.min(...values), maximum: Math.max(...values) };
}

function intersectRanges(ranges) {
  const intersection = {
    minimum: Math.max(...ranges.map((value) => value.minimum)),
    maximum: Math.min(...ranges.map((value) => value.maximum)),
  };
  assert(intersection.minimum <= intersection.maximum, "Ranges do not intersect");
  return intersection;
}

function parseArgs(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    if (!flag?.startsWith("--") || value === undefined) usage(1);
    parsed[flag.slice(2)] = value;
  }
  return parsed;
}

function required(parsed, key) {
  const value = parsed[key];
  if (value === undefined || value === "") throw new Error(`Missing --${key}`);
  return value;
}

function resolved(parsed, key) {
  const value = path.resolve(required(parsed, key));
  if (!existsSync(value)) throw new Error(`Missing ${key}: ${value}`);
  return value;
}

function resolvedDirectory(parsed, key) {
  const value = path.resolve(required(parsed, key));
  if (!existsSync(value) || !statSync(value).isDirectory()) {
    throw new Error(`Missing directory ${key}: ${value}`);
  }
  return value;
}

function scanExactIdTableDirectory(directory, id) {
  const needle = `"${id}"`;
  const files = readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort();
  let totalBytes = 0;
  let largestFileBytes = 0;
  const matchingFiles = [];
  for (const name of files) {
    const file = path.join(directory, name);
    const size = statSync(file).size;
    totalBytes += size;
    largestFileBytes = Math.max(largestFileBytes, size);
    if (readFileSync(file, "utf8").includes(needle)) matchingFiles.push(name);
  }
  return {
    path: directory.replaceAll("\\", "/"),
    exact_string_searched: needle,
    json_files_scanned: files.length,
    total_bytes_scanned: totalBytes,
    largest_file_bytes: largestFileBytes,
    one_file_materialized_at_a_time: true,
    matching_files: matchingFiles,
  };
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`Unable to read ${label} ${file}: ${error.message}`);
  }
}

async function descriptor(file) {
  const hash = createHash("sha256");
  const stream = createReadStream(file);
  for await (const chunk of stream) hash.update(chunk);
  return {
    path: file.replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: `sha256:${hash.digest("hex")}`,
  };
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return `sha256:${createHash("sha256").update(stableStringify(copy)).digest("hex")}`;
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function unique(values) {
  return [...new Set(values)];
}

function assertExact(actual, expected, label) {
  assert(JSON.stringify(actual) === JSON.stringify(expected), `${label} mismatch`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(code) {
  console.log("Usage:\n  node tools/bpsr-module-2300621-provider-scope-proof.mjs build --table-directory <Excels> --events <canonical.jsonl> --cohort <schema46.json> --counterfactual <schema20.json> --module-raw-165 <json> --module-raw-275 <json> --mod-effect-table <json> --affix-table <json> --buff-table <json> --affix-table-24568685 <json> --buff-table-24568685 <json> --affix-table-24609362 <json> --buff-table-24609362 <json> --damage-attr-table <json> --damage-stage-runtime <json> --damage-attr-formula-stage <json> --formula-runtime <json> --value-runtime <json> --community-modules-calc <modules_calc.js> --community-damage-calc <calc.js> --output <json>\n  node tools/bpsr-module-2300621-provider-scope-proof.mjs verify --input <json>\n  node tools/bpsr-module-2300621-provider-scope-proof.mjs self-test");
  process.exit(code);
}
