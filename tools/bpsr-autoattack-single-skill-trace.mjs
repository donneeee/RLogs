#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-autoattack-single-skill-trace.mjs";
const SCHEMA_VERSION = 13;
const GAME_BUILD = "24687926";
const AOYI_CONFIG_ID = 3_948;
const SUMMON_MONSTER_ID = 3_000_043;
const ABILITY_ID = 2_900_840;
const SKILL_EFFECT_ID = 290_084_001;
const EFFECT_ID = 2_110_140;
const ROUNDING_DISCRIMINANT_SESSION_ID = "monitor-1787002076016.run-0004";
const ROUNDING_DISCRIMINANT_PROVIDER_ENTITY_UUID = "5424024453760";
const ROUNDING_DISCRIMINANT_RECIPIENT_ENTITY_UUID = "216009015936";
const EXPECTED_HITS = [3, 5, 7, 8, 9];
const COEFFICIENT_EQUIVALENT_HITS = [7, 8, 9];
const EXPECTED_DAMAGE_ATTR_IDS = new Map([
  [3, 129_008_400_103],
  [5, 129_008_400_105],
  [7, 129_008_400_107],
  [8, 129_008_400_108],
  [9, 129_008_400_109],
]);

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(values) {
  const cohortPath = path.resolve(required(values, "cohort"));
  const counterfactualPath = path.resolve(required(values, "counterfactual"));
  const worklistPath = path.resolve(required(values, "static-worklist"));
  const inheritancePath = path.resolve(required(values, "inheritance-proof"));
  const decoderPath = path.resolve(required(values, "decoder-source"));
  const damageAttrPath = path.resolve(required(values, "damage-attr"));
  const skillEffectPath = path.resolve(required(values, "skill-effect"));
  const skillFightLevelPath = path.resolve(required(values, "skill-fight-level"));
  const skillAoyiPath = path.resolve(required(values, "skill-aoyi"));
  const il2cppDumpPath = path.resolve(required(values, "il2cpp-dump"));
  const skillVmBytecodePath = path.resolve(required(values, "skill-vm-bytecode"));
  const skillVmDecompiledPath = path.resolve(required(values, "skill-vm-decompiled"));
  const numberToolsBytecodePath = path.resolve(required(values, "number-tools-bytecode"));
  const numberToolsDecompiledPath = path.resolve(required(values, "number-tools-decompiled"));
  const enumDefineBytecodePath = path.resolve(required(values, "enum-define-bytecode"));
  const enumDefineDecompiledPath = path.resolve(required(values, "enum-define-decompiled"));
  const primaryAttackTransitionProofPath = path.resolve(
    required(values, "primary-attack-transition-proof"),
  );
  const familyTransitionSearchPath = path.resolve(
    required(values, "family-transition-search"),
  );
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const cohort = readJson(cohortPath);
  const counterfactual = readJson(counterfactualPath);
  const worklist = readJson(worklistPath);
  const inheritance = readJson(inheritancePath);
  const damageAttrs = readJson(damageAttrPath);
  const skillEffects = readJson(skillEffectPath);
  const skillFightLevels = readJson(skillFightLevelPath);
  const skillAoyi = readJson(skillAoyiPath);
  const primaryAttackTransitionProof = readJson(primaryAttackTransitionProofPath);
  const familyTransitionSearch = readJson(familyTransitionSearchPath);
  const il2cppDump = fs.readFileSync(il2cppDumpPath, "utf8");
  const skillVmDecompiled = fs.readFileSync(skillVmDecompiledPath, "utf8");
  const numberToolsDecompiled = fs.readFileSync(numberToolsDecompiledPath, "utf8");
  const enumDefineDecompiled = fs.readFileSync(enumDefineDecompiledPath, "utf8");
  const decoderSource = fs.readFileSync(decoderPath, "utf8");
  validateInputs(
    cohort,
    counterfactual,
    worklist,
    inheritance,
    decoderSource,
    familyTransitionSearch,
  );

  const staticRows = selectedStaticRows(worklist);
  const operatorStaticEvidence = autoattackFamilyOperatorEvidence(
    damageAttrs,
    skillEffects,
    staticRows,
  );
  const ownerEntityUuid = Number(
    inheritance.packet_observations.subject.owner_entity_uuid,
  );
  const samples = selectedSamples(cohort, ownerEntityUuid);
  const statLaneEvidence = autoattackStatLaneEvidence(samples, il2cppDump);
  const coefficientSelectionEvidence = exactAutoattackCoefficientSelectionEvidence(
    staticRows,
    samples,
    skillFightLevels,
    skillAoyi,
    skillVmDecompiled,
    enumDefineDecompiled,
  );
  const formulaPresentationEvidence = clientFormulaPresentationEvidence(
    skillVmDecompiled,
    numberToolsDecompiled,
  );
  const tier0RoundingDiscriminant = exactTier0RoundingDiscriminant(
    cohort,
    samples,
    staticRows,
    primaryAttackTransitionProof,
  );
  const statusCounts = summarizeEffectPresence(cohort, samples);
  const coefficientEquivalentGroups = compareCoefficientEquivalentStates(
    cohort,
    samples,
    false,
  );
  const coefficientEquivalentPositionGroups = compareCoefficientEquivalentStates(
    cohort,
    samples,
    true,
  );
  const strictRetainedInputGroups = compareStrictRetainedInputs(cohort, samples, false);
  const strictRetainedInputPositionGroups = compareStrictRetainedInputs(
    cohort,
    samples,
    true,
  );
  const positionCoverage = summarizePositionCoverage(samples);
  const sameWireCrossCoefficientBody = sameWireCrossCoefficientBodyDiagnostic(samples);

  const output = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    identity: {
      aoyi_config_id: AOYI_CONFIG_ID,
      summon_monster_id: SUMMON_MONSTER_ID,
      ability_id: ABILITY_ID,
      skill_effect_id: SKILL_EFFECT_ID,
      support_effect_id: EFFECT_ID,
      owner_entity_uuid: String(ownerEntityUuid),
      damage_script: "AutoAttack",
    },
    sources: {
      formula_cohort: fileReceipt(cohortPath),
      counterfactual_search: fileReceipt(counterfactualPath),
      damage_script_worklist: fileReceipt(worklistPath),
      summon_inheritance_proof: fileReceipt(inheritancePath),
      canonical_decoder_source: fileReceipt(decoderPath),
      current_build_damage_attr_table: fileReceipt(damageAttrPath),
      current_build_skill_effect_table: fileReceipt(skillEffectPath),
      current_build_skill_fight_level_table: fileReceipt(skillFightLevelPath),
      current_build_skill_aoyi_table: fileReceipt(skillAoyiPath),
      current_build_il2cpp_dump: fileReceipt(il2cppDumpPath),
      current_build_skill_vm_bytecode: fileReceipt(skillVmBytecodePath),
      current_build_skill_vm_decompiled: fileReceipt(skillVmDecompiledPath),
      current_build_number_tools_bytecode: fileReceipt(numberToolsBytecodePath),
      current_build_number_tools_decompiled: fileReceipt(numberToolsDecompiledPath),
      current_build_enum_define_bytecode: fileReceipt(enumDefineBytecodePath),
      current_build_enum_define_decompiled: fileReceipt(enumDefineDecompiledPath),
      current_build_primary_attack_transition_proof:
        fileReceipt(primaryAttackTransitionProofPath),
      current_build_autoattack_family_transition_search:
        fileReceipt(familyTransitionSearchPath),
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      current_character_snapshots_used: false,
      missing_remote_cast_packets_required: false,
      unresolved_effects_preserved: true,
      static_coefficients_are_complete_formula_authority: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
    static_route: {
      exact_aoyi_to_summon_to_skill_route: true,
      exact_skill_effect_route: true,
      exact_damage_attr_rows: staticRows,
      packet_hit_ids: EXPECTED_HITS,
      same_build_packet_occurrence_proven: true,
    },
    packet_route: {
      provider_status_recipient_edge_observed: true,
      owner_to_direct_summon_edge_observed: true,
      direct_summon_to_damage_target_edge_observed: true,
      direct_summon_attack_state_observed: false,
      omitted_inheritance_mode_semantics_proven: false,
      skill_effect_component_index_is_server_reported_formula_input: false,
      skill_effect_component_index_semantics:
        "zero-based local enumerate() ordinal over AoiSyncDelta.skill_effects.damage",
      skill_effect_component_count_semantics:
        "local AoiSyncDelta.skill_effects.damage array length",
      owner_attack_at_subject_spawn: inheritance.packet_observations.subject
        .owner_physical_attack_at_spawn,
    },
    cohort: {
      schema_version: cohort.schema_version,
      input_rlogs: cohort.inputs.length,
      selected_samples: samples.length,
      selected_sessions: [...new Set(samples.map((sample) => sample.session_id))].length,
      source_effect_presence: statusCounts,
      counterfactual_search: {
        schema_version: counterfactual.schema_version,
        exact_controlled_groups: counterfactual.summary.exact_controlled_groups,
        relaxed_controlled_groups: counterfactual.summary.relaxed_controlled_groups,
        near_controlled_source_pairs: counterfactual.summary.near_controlled_source_pairs,
        cross_entity_formula_state_controlled_groups:
          counterfactual.summary.cross_entity_formula_state_controlled_groups,
        reason: "every effect-present structural group lacks the same identity/formula state with the effect absent",
      },
    },
    autoattack_operator_frontier: {
      static_family_relation: operatorStaticEvidence,
      exact_stat_lane: statLaneEvidence,
      exact_coefficient_selection: coefficientSelectionEvidence,
      client_formula_presentation_semantics: formulaPresentationEvidence,
      tier0_rounding_discriminant: tier0RoundingDiscriminant,
      coefficient_equivalent_hit_ids: COEFFICIENT_EQUIVALENT_HITS,
      coefficient_equivalent_diagnostic_key: coefficientEquivalentKeyDescription(false),
      coefficient_equivalent_diagnostic_groups: coefficientEquivalentGroups.summary,
      coefficient_equivalent_position_diagnostic_key:
        coefficientEquivalentKeyDescription(true),
      coefficient_equivalent_position_diagnostic_groups:
        coefficientEquivalentPositionGroups.summary,
      exact_normalized_formula_input_key: strictRetainedInputKeyDescription(false),
      exact_normalized_formula_input_groups: strictRetainedInputGroups.summary,
      exact_normalized_formula_input_and_position_key:
        strictRetainedInputKeyDescription(true),
      exact_normalized_formula_input_and_position_groups:
        strictRetainedInputPositionGroups.summary,
      position_coverage: positionCoverage,
      same_wire_cross_coefficient_body_diagnostic: sameWireCrossCoefficientBody,
      family_transition_search: {
        schema_version: familyTransitionSearch.schema_version,
        input_files: familyTransitionSearch.summary.input_files,
        selected_samples: familyTransitionSearch.summary.selected_samples,
        samples_with_packet_observed_attack:
          familyTransitionSearch.summary.selected_samples -
          familyTransitionSearch.summary.missing_attack_samples,
        exact_multi_attack_groups:
          familyTransitionSearch.summary.multi_attack_groups,
        exact_candidate_pairs: familyTransitionSearch.summary.candidate_pairs,
        relaxed_source_state_pairs:
          familyTransitionSearch.summary.relaxed_source_state_pairs,
        cross_session_multi_attack_groups:
          familyTransitionSearch.summary.cross_session_multi_attack_groups,
        cross_session_pairs: familyTransitionSearch.summary.cross_session_pairs,
        packet_native_pre_mitigation_field_coverage:
          familyTransitionSearch.summary.packet_native_pre_mitigation_field_coverage,
        authority: "diagnostic_only_no_provider_credit",
      },
      diagnostic_divergent_examples:
        coefficientEquivalentPositionGroups.divergent_examples,
      interpretation: [
        "The exact current-build AutoAttack script family has 265 DamageAttr rows. Current-build SkillEffect text references 131 distinct AutoAttack rows; 124 are captured by an exact PVEDamageRadio x ATK/MATK + PVEFixedParameter formula pattern and the remaining seven are retained as unmatched evidence.",
        "All five exact ability-2900840 DamageAttr IDs are directly named by current-build SkillEffect formula text using the PVEDamageRadio x ATK/MATK + PVEFixedParameter relation. Ability 2900840's own SkillAttrDes text remains blank, so the exact row relation is proven by other current-build descriptions that reuse those numeric rows, not by a localized name or copied formula.",
        `All ${samples.length.toLocaleString("en-US")} selected owner-attributed ability-2900840 packets carry numeric damage_mode 1. Exact-build IL2CPP metadata defines EDamageMode.DamagePhysical as 1 and DamageMagical as 2, resolving ATK/MATK to packet-current Physical Attack attribute 11330 for this action.`,
        "Exact-build skill-description code selects one-based PVEDamageRadio level 1 for unremodeled Aoyi 3948. Its transformation rows are numeric type 1 (Attr), while exact enum_define.lua assigns SkillDamageMultiple numeric type 7, so no damage-multiple override applies. The exact coefficients are therefore 34500 for hits 3/5 and 27000 for hits 7/8/9.",
        "Packet owner_stage is perfectly correlated with hit identity (3->1, 5->2, 7/8/9->3) and is retained as event-stage evidence. It is not substituted for the exact level-1 coefficient selection proven by the current-build description path.",
        "Hits 7, 8, and 9 have identical current-build DamageAttr coefficient and fixed-parameter rows.",
        "A deliberately relaxed coefficient-equivalent diagnostic has 18 divergent groups after controlling retained owner/target attributes, statuses, stage, flags, packet position, and last packet-observed geometry.",
        "Every one of those 18 groups differs in canonical skill_effect_component_index, but decoder source proves this is a locally generated damage-array ordinal rather than a server-reported formula field.",
        "Hit-event IDs 7, 8, and 9 remain distinct exact numeric identities even though their static rows are identical. Once exact hit identity and complete retained provider/actor/packet context are controlled, there are zero repeated groups.",
        "Exact-build Lua proves that the formula token 'up' selects UnMarkAndPercentFormat: it divides the table integer by 100 and removes trailing display zeros. It is UI presentation semantics, not evidence of the server damage floor boundary.",
        "The exact tier-0 lifecycle changes internal Attack-add attribute 11332 by 298 but changes the damage formula's consumed current Physical Attack attribute 11330 by 346 in the same packet. A downstream damage projection must use the observed 346 current-Attack marginal for this lifecycle; using 298 directly would understate the provider contribution.",
        "Across the 45 exact ability-2900840 actions inside that lifecycle, floor, ceiling, nearest, and unrounded coefficient-stage candidates produce different integer projections. This is a sensitivity receipt, not attribution authority: downstream factor cancellation and the final integer boundary remain unproven.",
        "The expanded 23-RLOG cohort contains one same-wire group with both coefficient bodies, but its high-coefficient hit is non-critical while all low-coefficient hits are critical and packet type flags differ. There are zero cross-body same-wire pairs with matching critical, lucky, and type-flag context, so no shared hidden downstream factor is assumed.",
        `A separate family-wide pass examined ${familyTransitionSearch.summary.selected_samples.toLocaleString("en-US")} actions across all 197 exact current-build AutoAttack-linked abilities. Only ${(familyTransitionSearch.summary.selected_samples - familyTransitionSearch.summary.missing_attack_samples).toLocaleString("en-US")} actions had packet-observed source Attack; no exact or cross-session group changed Attack while preserving the required non-Attack source and target formula state. Four source-state-relaxed pairs were retained, but each changed many offensive attributes and statuses together and cannot adjudicate rounding.`,
        "None of the 64,796 family actions carries packet skill_effect_total_damage or actual_amount. The retained packet surface therefore has no server-reported pre-mitigation total that can bypass the controlled-pair or authoritative-operator requirement.",
        "The decoded coefficient row, coefficient-plus-fixed relation, and Physical Attack lane are proven inputs, but the exact server integer rounding remains unresolved; random roll, geometry, hit-specific server behavior, or another hidden stage are not invented.",
      ],
    },
    conclusion: {
      one_skill_identity_and_topology_reconstructed: true,
      shared_autoattack_coefficient_plus_fixed_relation_evidenced: true,
      exact_autoattack_row_coefficient_plus_fixed_relation_proven: true,
      exact_autoattack_operator_proven: false,
      exact_autoattack_stat_lane_proven: true,
      exact_integer_rounding_proven: false,
      exact_provider_counterfactual_proven: false,
      observed_damage_conservation_changed: false,
      promotion_decision: "fail_closed_no_provider_rdps_credit",
      smallest_next_proof: "The retained 23-RLOG corpus is now exhausted both for ability 2900840 and across all 197 exact-build AutoAttack-linked abilities. Capture a controlled same-hit ability-2900840 repeat spanning a proven current-Attack transition while retaining identical downstream target and hit context, or recover an authoritative server operator. Remote-player cast packets are not required.",
    },
  };
  output.content_sha256 = contentHash(output);
  verifyReport(output);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify(consoleSummary(output), null, 2)}\n`);
}

function validateInputs(
  cohort,
  counterfactual,
  worklist,
  inheritance,
  decoderSource,
  familyTransitionSearch,
) {
  assert.equal(cohort.schema_version, 46);
  assert.ok(
    cohort.samples.every((sample) =>
      Object.hasOwn(sample, "source_position_at_wire_message_start") &&
      Object.hasOwn(sample, "direct_source_position_at_wire_message_start") &&
      Object.hasOwn(sample, "target_position_at_wire_message_start")
    ),
    "schema-45 cohort must retain explicit source, direct-source, and target position fields",
  );
  assert.equal(String(cohort.game_build), GAME_BUILD);
  assert.ok(Array.isArray(cohort.inputs) && cohort.inputs.length > 0);
  assert.ok(Array.isArray(cohort.samples) && cohort.samples.length > 0);
  assert.equal(Number(cohort.selection?.ability_id ?? cohort.selection?.ability_ids?.[0]), ABILITY_ID);

  assert.equal(counterfactual.schema_version, 18);
  assert.equal(String(counterfactual.game_build), GAME_BUILD);
  assert.equal(counterfactual.effects.length, 1);
  assert.equal(counterfactual.effects[0].locus, "source");
  assert.equal(counterfactual.effects[0].effect_id, EFFECT_ID);

  assert.equal(String(worklist.game_build), GAME_BUILD);
  assert.ok(Array.isArray(worklist.families));
  assert.equal(inheritance.schema_version, 1);
  assert.equal(String(inheritance.identity?.game_build), GAME_BUILD);
  assert.equal(inheritance.static_identity_route.aoyi_config_id, AOYI_CONFIG_ID);
  assert.equal(inheritance.static_identity_route.aoyi_monster_id, SUMMON_MONSTER_ID);
  assert.deepEqual(inheritance.static_identity_route.monster_skill_ids, [ABILITY_ID]);
  assert.deepEqual(inheritance.static_identity_route.skill_effect_ids, [SKILL_EFFECT_ID]);
  assert.equal(inheritance.policy.provider_credit_allowed, false);
  assert.equal(inheritance.policy.runtime_promotion_allowed, false);
  assert.match(decoderSource, /effect\.damage\.into_iter\(\)\.enumerate\(\)/);
  assert.match(
    decoderSource,
    /skill_effect_component_index:\s*u32::try_from\(component_index\)\.ok\(\)/,
  );
  assert.match(
    decoderSource,
    /skill_effect_component_count\s*=\s*u32::try_from\(effect\.damage\.len\(\)\)\.ok\(\)/,
  );
  assert.equal(familyTransitionSearch.schema_version, 4);
  assert.equal(String(familyTransitionSearch.game_build), GAME_BUILD);
  assert.equal(familyTransitionSearch.generated_by, "tools/bpsr-autoattack-family-transition-search.mjs");
  assert.equal(familyTransitionSearch.policy.provider_rdps_credit_allowed, false);
  assert.equal(familyTransitionSearch.conclusion.runtime_promotion_allowed, false);
}

function selectedStaticRows(worklist) {
  const family = worklist.families.find((entry) => entry.damage_script === "AutoAttack");
  assert.ok(family, "current-build worklist has no AutoAttack family");
  const rows = [];
  for (const signature of family.formula_signatures ?? []) {
    for (const item of signature.work_items ?? []) {
      if (Number(item.ability_id) !== ABILITY_ID) continue;
      const row = item.damage_attr;
      rows.push({
        hit_event_id: Number(item.hit_event_id),
        damage_attr_id: Number(row.damage_attr_id),
        damage_type: Number(row.damage_type),
        damage_property: Number(row.damage_property),
        coefficient_basis_points_by_stage: row.coefficient_basis_points_by_stage,
        fixed_parameter_by_level: row.fixed_parameter_by_level,
        damage_script: row.damage_script,
        static_route_count: item.static_routes.length,
      });
    }
  }
  rows.sort((left, right) => left.hit_event_id - right.hit_event_id);
  assert.deepEqual(rows.map((row) => row.hit_event_id), EXPECTED_HITS);
  assert.ok(rows.every((row) => row.damage_script === "AutoAttack"));
  assert.deepEqual(rows[2].coefficient_basis_points_by_stage, rows[3].coefficient_basis_points_by_stage);
  assert.deepEqual(rows[3].coefficient_basis_points_by_stage, rows[4].coefficient_basis_points_by_stage);
  assert.deepEqual(rows[2].fixed_parameter_by_level, rows[3].fixed_parameter_by_level);
  assert.deepEqual(rows[3].fixed_parameter_by_level, rows[4].fixed_parameter_by_level);
  return rows;
}

function autoattackFamilyOperatorEvidence(damageAttrs, skillEffects, staticRows) {
  assert.ok(damageAttrs && typeof damageAttrs === "object");
  assert.ok(skillEffects && typeof skillEffects === "object");
  const autoattackRows = new Map(
    Object.entries(damageAttrs)
      .filter(([, row]) => row?.DamageScript === "AutoAttack")
      .map(([id, row]) => [Number(id), row]),
  );
  assert.ok(autoattackRows.size > 0, "decoded table has no AutoAttack rows");

  const directTargetRows = staticRows.map((selected) => {
    const expectedId = EXPECTED_DAMAGE_ATTR_IDS.get(selected.hit_event_id);
    assert.equal(selected.damage_attr_id, expectedId);
    const row = autoattackRows.get(expectedId);
    assert.ok(row, `target DamageAttr row ${expectedId} is not AutoAttack`);
    assert.equal(Number(row.Id), expectedId);
    assert.equal(Number(row.TypeEnum), ABILITY_ID);
    assert.equal(Number(row.DamageType), selected.damage_type);
    assert.deepEqual(row.PVEDamageRadio, selected.coefficient_basis_points_by_stage);
    assert.deepEqual(row.PVEFixedParameter, selected.fixed_parameter_by_level);
    return {
      hit_event_id: selected.hit_event_id,
      damage_attr_id: expectedId,
      damage_type: Number(row.DamageType),
      coefficient_basis_points_by_stage: row.PVEDamageRadio,
      fixed_parameter_by_level: row.PVEFixedParameter,
    };
  });

  const formulaMatches = [];
  const allReferencedAutoattackIds = new Set();
  const formulaReferencedAutoattackIds = new Set();
  for (const [skillEffectId, row] of Object.entries(skillEffects)) {
    for (const entry of row?.SkillAttrDes ?? []) {
      const text = Array.isArray(entry) ? String(entry[1] ?? "") : "";
      for (const id of numericTokens(text)) {
        if (autoattackRows.has(id)) allReferencedAutoattackIds.add(id);
      }
      for (const match of autoattackFormulaMatches(text)) {
        const fixedIds = new Set(match.fixed_damage_attr_ids);
        const referenced = match.coefficient_damage_attr_ids.filter(
          (id) => fixedIds.has(id) && autoattackRows.has(id),
        );
        if (referenced.length === 0) continue;
        for (const id of referenced) formulaReferencedAutoattackIds.add(id);
        formulaMatches.push({
          skill_effect_id: Number(skillEffectId),
          label: String(entry[0] ?? ""),
          coefficient_damage_attr_ids: match.coefficient_damage_attr_ids,
          coefficient_hit_multipliers: match.coefficient_hit_multipliers,
          fixed_damage_attr_ids: match.fixed_damage_attr_ids,
          fixed_hit_multipliers: match.fixed_hit_multipliers,
          arrays_align_exactly: match.arrays_align_exactly,
          autoattack_damage_attr_ids: referenced,
          relation: "PVEDamageRadio x ATK/MATK + PVEFixedParameter",
        });
      }
    }
  }

  const directTargetDescriptionIds = directTargetRows
    .map((row) => row.damage_attr_id)
    .filter((id) => formulaReferencedAutoattackIds.has(id));
  const directTargetIdSet = new Set(directTargetRows.map((row) => row.damage_attr_id));
  const exactAbilityFormulaOccurrences = formulaMatches.filter((match) =>
    match.autoattack_damage_attr_ids.some((id) => directTargetIdSet.has(id))
  );
  const ownSkillEffectFormulaOccurrences = exactAbilityFormulaOccurrences.filter(
    (match) => match.skill_effect_id === SKILL_EFFECT_ID,
  );
  const unmatchedReferencedIds = [...allReferencedAutoattackIds]
    .filter((id) => !formulaReferencedAutoattackIds.has(id))
    .sort((left, right) => left - right);
  const damageTypeCounts = [...autoattackRows.values()]
    .reduce((counts, row) => {
      const key = String(row.DamageType);
      counts.set(key, (counts.get(key) ?? 0) + 1);
      return counts;
    }, new Map());

  return {
    exact_script_key: "AutoAttack",
    coefficient_plus_fixed_relation_proven_for_description_linked_rows: true,
    shared_script_key_relation_candidate_for_exact_ability_rows: true,
    relation: "PVEDamageRadio x ATK/MATK + PVEFixedParameter",
    exact_integer_rounding_proven: false,
    exact_stat_lane_proven_for_ability_2900840: true,
    autoattack_damage_attr_rows: autoattackRows.size,
    damage_type_counts: Object.fromEntries(
      [...damageTypeCounts].sort(([left], [right]) => Number(left) - Number(right)),
    ),
    skill_effect_formula_occurrences: formulaMatches.length,
    skill_effect_formula_occurrences_with_exact_array_alignment:
      formulaMatches.filter((match) => match.arrays_align_exactly).length,
    formula_referenced_autoattack_rows: formulaReferencedAutoattackIds.size,
    all_skill_description_referenced_autoattack_rows: allReferencedAutoattackIds.size,
    unmatched_skill_description_referenced_autoattack_rows: unmatchedReferencedIds,
    exact_ability_rows_with_direct_formula_text: directTargetDescriptionIds,
    exact_ability_formula_occurrences: exactAbilityFormulaOccurrences,
    own_skill_effect_formula_occurrences: ownSkillEffectFormulaOccurrences.length,
    exact_ability_rows: directTargetRows,
    representative_formula_occurrences: formulaMatches.slice(0, 12),
    authority_boundary: [
      "Exact numeric DamageScript is the shared operator-family key; localized labels are retained evidence only.",
      "Current-build formula text proves coefficient multiplication followed by fixed-parameter addition for the shared AutoAttack family.",
      "The formula text alone does not select ATK or MATK, but packet damage_mode plus the exact-build numeric enum independently resolves ability 2900840 to Physical Attack.",
      "All five exact DamageAttr IDs are directly referenced by current-build formula text, while ability 2900840's own SkillEffect row has no formula occurrence.",
      "Direct row text closes coefficient multiplication followed by fixed addition. Packet and exact-build enum evidence close the Physical Attack lane; the server integer rounding boundary remains separate and unresolved.",
    ],
  };
}

function numericTokens(text) {
  return [...String(text).matchAll(/(?<!\d)\d{6,}(?!\d)/g)].map((match) => Number(match[0]));
}

function autoattackFormulaMatches(text) {
  const pattern = /\{\*skillpara\.damageMerge\(\{([0-9,\s]+)\},\{([0-9,\s]+)\},"PVEDamageRadio","up"\)\*\}\s*ATK\/MATK\s*\+\s*\{\*skillpara\.damageMerge\(\{([0-9,\s]+)\},\{([0-9,\s]+)\},"PVEFixedParameter","un"\)\*\}/g;
  const matches = [];
  for (const match of String(text).matchAll(pattern)) {
    const coefficientDamageAttrIds = integerList(match[1]);
    const coefficientHitMultipliers = integerList(match[2]);
    const fixedDamageAttrIds = integerList(match[3]);
    const fixedHitMultipliers = integerList(match[4]);
    matches.push({
      coefficient_damage_attr_ids: coefficientDamageAttrIds,
      coefficient_hit_multipliers: coefficientHitMultipliers,
      fixed_damage_attr_ids: fixedDamageAttrIds,
      fixed_hit_multipliers: fixedHitMultipliers,
      arrays_align_exactly:
        stableStringify(coefficientDamageAttrIds) === stableStringify(fixedDamageAttrIds) &&
        stableStringify(coefficientHitMultipliers) === stableStringify(fixedHitMultipliers) &&
        coefficientDamageAttrIds.length === coefficientHitMultipliers.length,
    });
  }
  return matches;
}

function integerList(value) {
  return value.split(",").map((entry) => Number(entry.trim()));
}

function autoattackStatLaneEvidence(samples, il2cppDump) {
  assert.match(
    il2cppDump,
    /public enum EDamageMode[\s\S]*?DamageNormal\s*=\s*0;[\s\S]*?DamagePhysical\s*=\s*1;[\s\S]*?DamageMagical\s*=\s*2;/,
  );
  const damageModeCounts = new Map();
  for (const sample of samples) {
    const mode = Number(sample.packet?.damage_mode);
    assert.ok(Number.isInteger(mode), "selected sample lacks packet damage_mode");
    damageModeCounts.set(mode, (damageModeCounts.get(mode) ?? 0) + 1);
  }
  assert.deepEqual([...damageModeCounts.keys()], [1]);
  return {
    exact_stat_lane_proven: true,
    selected_stat: "Physical Attack",
    selected_attribute_id: 11_330,
    packet_damage_mode: 1,
    packet_damage_mode_counts: Object.fromEntries(damageModeCounts),
    exact_current_build_enum: {
      type: "Zproto.EDamageMode",
      normal: 0,
      physical: 1,
      magical: 2,
    },
    proof:
      "Every selected ability-2900840 packet carries numeric damage_mode 1. Exact-build IL2CPP metadata defines EDamageMode.DamagePhysical as 1 and DamageMagical as 2, resolving the exact-row formula text's ATK/MATK branch to packet-current Physical Attack attribute 11330.",
  };
}

function exactAutoattackCoefficientSelectionEvidence(
  staticRows,
  samples,
  skillFightLevels,
  skillAoyi,
  skillVmSource,
  enumDefineSource,
) {
  const fightLevel = skillFightLevels[String(394_801)];
  assert.equal(Number(fightLevel?.Id), 394_801);
  assert.equal(Number(fightLevel?.SkillId), AOYI_CONFIG_ID);
  assert.equal(Number(fightLevel?.Level), 1);
  assert.equal(Number(fightLevel?.SkillEffectId), 394_801);
  const aoyi = skillAoyi[String(AOYI_CONFIG_ID)];
  assert.equal(Number(aoyi?.Id), AOYI_CONFIG_ID);
  assert.equal(Number(aoyi?.MonsterId), SUMMON_MONSTER_ID);
  assert.match(
    enumDefineSource,
    /E\.RemodelInfoType\s*=\s*\{[\s\S]*?Attr\s*=\s*1,[\s\S]*?SkillDamageMultiple\s*=\s*7,/,
  );
  const transformationTypes = (aoyi.TransformationType ?? []).map((row) => Number(row[0]));
  assert.ok(transformationTypes.length > 0);
  assert.ok(transformationTypes.every((type) => type === 1));
  assert.ok(!transformationTypes.includes(7));
  assert.match(skillVmSource, /level\s*=\s*remodelLevel2\s*<=\s*0\s*and\s*1\s*or\s*level/);
  assert.match(
    skillVmSource,
    /if\s+r\d+_\d+\s*==\s*E\.RemodelInfoType\.SkillDamageMultiple\s*then[\s\S]*?level\s*=\s*r\d+_\d+\s*\+\s*1/,
  );
  assert.match(skillVmSource, /damageAttrTableRow\[tableHeardName\]\[level\]/);

  const selectedCoefficients = Object.fromEntries(staticRows.map((row) => [
    String(row.hit_event_id),
    row.coefficient_basis_points_by_stage[0],
  ]));
  assert.deepEqual(selectedCoefficients, {
    "3": 34_500,
    "5": 34_500,
    "7": 27_000,
    "8": 27_000,
    "9": 27_000,
  });
  const ownerStageByHit = {};
  for (const sample of samples) {
    const hit = String(Number(sample.hit_event_id));
    const stage = String(Number(sample.packet?.owner_stage));
    ownerStageByHit[hit] ??= {};
    ownerStageByHit[hit][stage] = (ownerStageByHit[hit][stage] ?? 0) + 1;
  }
  const expectedStageByHit = { "3": "1", "5": "2", "7": "3", "8": "3", "9": "3" };
  for (const [hit, expectedStage] of Object.entries(expectedStageByHit)) {
    assert.deepEqual(Object.keys(ownerStageByHit[hit] ?? {}), [expectedStage]);
  }
  assert.equal(
    Object.values(ownerStageByHit).reduce(
      (total, stages) => total + Object.values(stages).reduce(
        (stageTotal, count) => stageTotal + count,
        0,
      ),
      0,
    ),
    samples.length,
  );
  return {
    exact_description_skill_fight_level_id: 394_801,
    exact_description_level: 1,
    remodel_level: 0,
    skill_damage_multiple_enum_value: 7,
    observed_aoyi_transformation_type_values: [...new Set(transformationTypes)].sort(),
    damage_multiple_override_applies: false,
    pve_damage_ratio_indexing: "one-based level 1",
    selected_coefficient_basis_points_by_hit_event_id: selectedCoefficients,
    packet_owner_stage_counts_by_hit_event_id: ownerStageByHit,
    packet_owner_stage_used_as_coefficient_index: false,
    exact_coefficient_selection_proven: true,
    authority_boundary:
      "Exact current-build tables and the skill-description selection path prove the row coefficient displayed for unremodeled Aoyi 3948. Packet owner_stage is retained as a distinct event-stage field and is not silently reused as the coefficient-array index.",
  };
}

function clientFormulaPresentationEvidence(skillVmSource, numberToolsSource) {
  assert.match(
    skillVmSource,
    /up\s*=\s*numberTools\.UnMarkAndPercentFormat/,
    "skill_vm does not map the exact 'up' token to UnMarkAndPercentFormat",
  );
  assert.match(
    skillVmSource,
    /damageMerge\s*=\s*function[\s\S]*?num\s*=\s*num\s*\+\s*damageAttrTableRow\[tableHeardName\]\[level\]\s*\*\s*multiple[\s\S]*?ret\.formatFloatParam\(num,\s*formatType\)/,
    "skill_vm does not prove damageMerge aggregation followed by format dispatch",
  );
  assert.match(
    numberToolsSource,
    /function\s+ret\.UnMarkAndPercentFormat\(value\)[\s\S]*?local\s+v\s*=\s*value\s*\/\s*100[\s\S]*?ret\.removeTrailingZeros\(v\)[\s\S]*?Lang\("Percent"/,
    "number_tools does not prove the 'up' display transformation",
  );
  return {
    exact_formula_format_token: "up",
    exact_format_function: "utility.number_tools.UnMarkAndPercentFormat",
    aggregate_before_format: true,
    display_transform: "value / 100, remove trailing display zeros, render Percent localization",
    server_integer_rounding_proven: false,
    authority_boundary:
      "The exact-build Lua path is a skill-description renderer. It proves that 'up' is unmarked percent presentation and must not be interpreted as ceil/up-rounding or as the server damage integer boundary.",
  };
}

function exactTier0RoundingDiscriminant(
  cohort,
  samples,
  staticRows,
  transitionProof,
) {
  assert.equal(transitionProof.schema_version, 1);
  assert.equal(String(transitionProof.game_build), GAME_BUILD);
  assert.equal(Number(transitionProof.effect_id), EFFECT_ID);
  assert.equal(transitionProof.policy?.provider_rdps_credit_allowed, false);
  assert.equal(transitionProof.policy?.runtime_authority, false);

  const windows = transitionProof.lifecycle_windows.filter(
    (window) =>
      window.session_id === ROUNDING_DISCRIMINANT_SESSION_ID &&
      String(window.provider_entity_uuid) ===
        ROUNDING_DISCRIMINANT_PROVIDER_ENTITY_UUID &&
      String(window.affected_entity_uuid) ===
        ROUNDING_DISCRIMINANT_RECIPIENT_ENTITY_UUID &&
      Number(window.loadout_tier) === 0,
  );
  assert.equal(windows.length, 1, "expected one exact tier-0 lifecycle window");
  const window = windows[0];
  const activation = window.activation;
  const deactivation = window.deactivation;
  assert.equal(activation.join_candidate_count, 1);
  assert.equal(deactivation.join_candidate_count, 1);
  assert.equal(
    activation.transition.actor_entity_uuid,
    ROUNDING_DISCRIMINANT_RECIPIENT_ENTITY_UUID,
  );
  assert.equal(
    deactivation.transition.actor_entity_uuid,
    ROUNDING_DISCRIMINANT_RECIPIENT_ENTITY_UUID,
  );
  assert.equal(activation.retained_family_members.attack_add.delta, 298);
  assert.equal(deactivation.retained_family_members.attack_add.delta, -298);
  assert.equal(activation.retained_family_members.attack_current.delta, 346);
  assert.equal(deactivation.retained_family_members.attack_current.delta, -346);
  assert.deepEqual(activation.retained_family_members.other_same_packet_changes, []);
  assert.deepEqual(deactivation.retained_family_members.other_same_packet_changes, []);
  assert.equal(
    activation.packet_transform_checks.primary_current_to_attack_add_58_over_100.exact,
    true,
  );
  assert.equal(
    deactivation.packet_transform_checks.primary_current_to_attack_add_58_over_100.exact,
    true,
  );

  const firstExclusiveSequence = Number(
    window.effective_stat_window.first_exclusive_canonical_source_rlog_sequence,
  );
  const lastExclusiveSequence = Number(
    window.effective_stat_window.last_exclusive_canonical_source_rlog_sequence,
  );
  const currentAttackDelta = Number(
    activation.retained_family_members.attack_current.delta,
  );
  const rowsByHit = new Map(staticRows.map((row) => [row.hit_event_id, row]));
  const lifecycleSamples = samples.filter((sample) => {
    if (sample.session_id !== ROUNDING_DISCRIMINANT_SESSION_ID) return false;
    if (String(sample.source_entity_uuid) !== ROUNDING_DISCRIMINANT_RECIPIENT_ENTITY_UUID) {
      return false;
    }
    if (!(sample.sequence > firstExclusiveSequence && sample.sequence < lastExclusiveSequence)) {
      return false;
    }
    return statusState(cohort, sample.source_status_state_id).some(
      (effect) =>
        Number(effect.effect_id) === EFFECT_ID &&
        String(effect.source_entity_uuid) ===
          ROUNDING_DISCRIMINANT_PROVIDER_ENTITY_UUID,
    );
  });
  assert.equal(lifecycleSamples.length, 45);

  const modes = ["floor", "ceil", "nearest_half_up", "unrounded_rational"];
  const projectedTotals = Object.fromEntries(modes.map((mode) => [mode, 0]));
  const marginalValues = Object.fromEntries(modes.map((mode) => [mode, new Set()]));
  const divergentExamples = [];
  let allModesEqual = 0;
  let floorCeilingEqual = 0;

  for (const sample of lifecycleSamples) {
    const row = rowsByHit.get(Number(sample.hit_event_id));
    assert.ok(row, `missing exact row for hit ${sample.hit_event_id}`);
    const coefficientBasisPoints = Number(row.coefficient_basis_points_by_stage[0]);
    const fixedParameter = Number(row.fixed_parameter_by_level[0]);
    const currentAttack = attributeValue(
      cohort,
      sample.source_attribute_state_id,
      11_330,
    );
    assert.ok(currentAttack > currentAttackDelta);
    const projections = Object.fromEntries(modes.map((mode) => {
      const value = coefficientStageProjection({
        observedAmount: Number(sample.amount),
        currentAttack,
        currentAttackDelta,
        coefficientBasisPoints,
        fixedParameter,
        mode,
      });
      projectedTotals[mode] += value.projected_provider_damage;
      marginalValues[mode].add(value.marginal_key);
      return [mode, value];
    }));
    const projected = modes.map(
      (mode) => projections[mode].projected_provider_damage,
    );
    if (new Set(projected).size === 1) allModesEqual += 1;
    if (
      projections.floor.projected_provider_damage ===
      projections.ceil.projected_provider_damage
    ) {
      floorCeilingEqual += 1;
    } else {
      divergentExamples.push({
        sequence: Number(sample.sequence),
        hit_event_id: Number(sample.hit_event_id),
        observed_amount: Number(sample.amount),
        current_physical_attack_11330: currentAttack,
        candidates: Object.fromEntries(modes.map((mode) => [
          mode,
          compactProjection(projections[mode]),
        ])),
      });
    }
  }

  const totalValues = Object.values(projectedTotals);
  return {
    authority: "diagnostic_only_no_provider_credit",
    exact_lifecycle_identity: {
      session_id: ROUNDING_DISCRIMINANT_SESSION_ID,
      effect_id: EFFECT_ID,
      status_instance_id: Number(window.status_instance_id),
      provider_entity_uuid: ROUNDING_DISCRIMINANT_PROVIDER_ENTITY_UUID,
      recipient_entity_uuid: ROUNDING_DISCRIMINANT_RECIPIENT_ENTITY_UUID,
      loadout_tier: 0,
      first_exclusive_sequence: firstExclusiveSequence,
      last_exclusive_sequence: lastExclusiveSequence,
    },
    exact_same_packet_attribute_transition: {
      consumed_damage_formula_attribute_id: 11_330,
      current_physical_attack_delta: currentAttackDelta,
      internal_attack_add_attribute_id: 11_332,
      internal_attack_add_delta: 298,
      current_attack_delta_used_for_damage_projection: true,
      internal_attack_add_delta_used_directly_for_damage_projection: false,
      proof:
        "The exact lifecycle packet changes current Physical Attack 11330 by +346 and internal Attack-add 11332 by +298 with no other same-packet family changes. Because the exact AutoAttack lane consumes 11330, this lifecycle's diagnostic damage projection removes 346 from 11330; it does not substitute the internal 298 member.",
    },
    exact_selected_actions: lifecycleSamples.length,
    diagnostic_projection_contract:
      "floor(observed_damage * candidate_coefficient_stage_marginal / candidate_active_coefficient_stage_base)",
    diagnostic_assumption:
      "All unobserved downstream factors are multiplicative and identical between active and counterfactual damage, so they cancel in the ratio. This is not yet proven and is why every result remains diagnostic only.",
    candidate_coefficient_stage_boundaries: {
      floor: "floor(attack * coefficient / 10000) + fixed",
      ceil: "ceil(attack * coefficient / 10000) + fixed",
      nearest_half_up:
        "floor((attack * coefficient + 5000) / 10000) + fixed for nonnegative integers",
      unrounded_rational:
        "(attack * coefficient + fixed * 10000) / 10000",
    },
    projected_provider_damage_totals: projectedTotals,
    projected_total_minimum: Math.min(...totalValues),
    projected_total_maximum: Math.max(...totalValues),
    projected_total_spread: Math.max(...totalValues) - Math.min(...totalValues),
    marginal_values_by_candidate: Object.fromEntries(modes.map((mode) => [
      mode,
      [...marginalValues[mode]].sort(),
    ])),
    actions_with_all_four_candidate_projections_equal: allModesEqual,
    actions_with_floor_and_ceiling_projection_equal: floorCeilingEqual,
    actions_with_floor_and_ceiling_projection_different:
      lifecycleSamples.length - floorCeilingEqual,
    floor_ceiling_divergent_examples: divergentExamples,
    exact_server_integer_rounding_proven: false,
    downstream_factor_cancellation_proven: false,
    provider_rdps_credit_allowed: false,
    interpretation:
      "The corrected consumed-stat marginal makes the unresolved operator consequential: floor, ceiling, nearest-half-up, and unrounded-rational candidates total differently, and floor versus ceiling differs on 26 of 45 actions. A controlled same-hit transition or authoritative server operator must select the boundary before attribution.",
  };
}

function coefficientStageProjection({
  observedAmount,
  currentAttack,
  currentAttackDelta,
  coefficientBasisPoints,
  fixedParameter,
  mode,
}) {
  const denominator = 10_000;
  const activeNumerator = currentAttack * coefficientBasisPoints;
  const inactiveNumerator =
    (currentAttack - currentAttackDelta) * coefficientBasisPoints;
  if (mode === "unrounded_rational") {
    const activeBaseNumerator = activeNumerator + fixedParameter * denominator;
    const inactiveBaseNumerator = inactiveNumerator + fixedParameter * denominator;
    const marginalNumerator = activeBaseNumerator - inactiveBaseNumerator;
    return {
      active_base_numerator: activeBaseNumerator,
      inactive_base_numerator: inactiveBaseNumerator,
      denominator,
      marginal_numerator: marginalNumerator,
      marginal_key: `${marginalNumerator}/${denominator}`,
      projected_provider_damage: Math.floor(
        observedAmount * marginalNumerator / activeBaseNumerator,
      ),
    };
  }
  const activeBase = roundPositiveRational(activeNumerator, denominator, mode) +
    fixedParameter;
  const inactiveBase = roundPositiveRational(inactiveNumerator, denominator, mode) +
    fixedParameter;
  const marginal = activeBase - inactiveBase;
  return {
    active_base: activeBase,
    inactive_base: inactiveBase,
    marginal,
    marginal_key: String(marginal),
    projected_provider_damage: Math.floor(observedAmount * marginal / activeBase),
  };
}

function roundPositiveRational(numerator, denominator, mode) {
  assert.ok(Number.isSafeInteger(numerator) && numerator >= 0);
  assert.ok(Number.isSafeInteger(denominator) && denominator > 0);
  if (mode === "floor") return Math.floor(numerator / denominator);
  if (mode === "ceil") return Math.ceil(numerator / denominator);
  if (mode === "nearest_half_up") {
    return Math.floor((numerator + denominator / 2) / denominator);
  }
  throw new Error(`unsupported coefficient-stage rounding candidate: ${mode}`);
}

function compactProjection(projection) {
  const { marginal_key: _marginalKey, ...rest } = projection;
  return rest;
}

function attributeValue(cohort, stateId, attributeId) {
  const value = cohort.attribute_states[stateId]?.find(
    (attribute) => Number(attribute.attribute_id) === attributeId,
  )?.value;
  assert.ok(Number.isSafeInteger(Number(value)),
    `attribute state ${stateId} lacks integer attribute ${attributeId}`);
  return Number(value);
}

function selectedSamples(cohort, ownerEntityUuid) {
  const samples = cohort.samples.filter(
    (sample) =>
      Number(sample.ability_id) === ABILITY_ID &&
      Number(sample.source_entity_uuid) === ownerEntityUuid &&
      Number(sample.direct_source_actor_identity?.monster_id) === SUMMON_MONSTER_ID,
  );
  assert.ok(samples.length > 0);
  assert.deepEqual(
    [...new Set(samples.map((sample) => Number(sample.hit_event_id)))].sort((a, b) => a - b),
    EXPECTED_HITS,
  );
  return samples;
}

function sameWireCrossCoefficientBodyDiagnostic(samples) {
  const groups = new Map();
  for (const sample of samples) {
    const key = stableStringify([
      sample.session_id,
      sample.run_ordinal,
      sample.wire_capture_sequence,
      String(sample.source_entity_uuid),
      String(sample.direct_source_entity_uuid),
      String(sample.target_entity_uuid),
    ]);
    const group = groups.get(key) ?? [];
    group.push(sample);
    groups.set(key, group);
  }
  let multipleHitWireGroups = 0;
  let crossCoefficientBodyWireGroups = 0;
  let matchedFlagsCrossBodyPairs = 0;
  const examples = [];
  for (const group of groups.values()) {
    const hits = new Set(group.map((sample) => Number(sample.hit_event_id)));
    if (hits.size > 1) multipleHitWireGroups += 1;
    const high = group.filter((sample) => [3, 5].includes(Number(sample.hit_event_id)));
    const low = group.filter((sample) => [7, 8, 9].includes(Number(sample.hit_event_id)));
    if (high.length === 0 || low.length === 0) continue;
    crossCoefficientBodyWireGroups += 1;
    for (const highSample of high) {
      for (const lowSample of low) {
        if (
          highSample.critical === lowSample.critical &&
          highSample.lucky === lowSample.lucky &&
          highSample.packet?.type_flags === lowSample.packet?.type_flags
        ) {
          matchedFlagsCrossBodyPairs += 1;
        }
      }
    }
    if (examples.length < 8) {
      examples.push(group.map((sample) => ({
        session_id: sample.session_id,
        wire_capture_sequence: Number(sample.wire_capture_sequence),
        sequence: Number(sample.sequence),
        hit_event_id: Number(sample.hit_event_id),
        amount: Number(sample.amount),
        critical: sample.critical,
        lucky: sample.lucky,
        type_flags: sample.packet?.type_flags,
        owner_stage: sample.packet?.owner_stage,
        skill_effect_component_index: sample.packet?.skill_effect_component_index,
        skill_effect_component_count: sample.packet?.skill_effect_component_count,
      })));
    }
  }
  return {
    selected_wire_groups: groups.size,
    multiple_hit_wire_groups: multipleHitWireGroups,
    cross_coefficient_body_wire_groups: crossCoefficientBodyWireGroups,
    cross_body_pairs_with_matching_critical_lucky_and_type_flags:
      matchedFlagsCrossBodyPairs,
    examples,
    shared_downstream_factor_proven: false,
    authority_boundary:
      "One wire contains both exact coefficient bodies, but its high-body hit is non-critical while every low-body hit is critical and type flags differ. With zero cross-body pairs matching critical, lucky, and type flags, the wire cannot identify a shared downstream factor or integer boundary.",
  };
}

function summarizeEffectPresence(cohort, samples) {
  let present = 0;
  let absent = 0;
  const providers = new Set();
  for (const sample of samples) {
    const effect = statusState(cohort, sample.source_status_state_id)
      .find((status) => Number(status.effect_id) === EFFECT_ID);
    if (effect) {
      present += 1;
      providers.add(String(effect.source_entity_uuid));
    } else absent += 1;
  }
  assert.ok(present > 0 && absent > 0);
  return { present_samples: present, absent_samples: absent, provider_entity_uuids: [...providers].sort() };
}

function compareCoefficientEquivalentStates(cohort, samples, includeObservedPositions) {
  return compareGroups(
    cohort,
    samples,
    (sample) => coefficientEquivalentKey(sample, includeObservedPositions),
  );
}

function compareStrictRetainedInputs(cohort, samples, includeObservedPositions) {
  return compareGroups(
    cohort,
    samples,
    (sample) => strictRetainedInputKey(sample, includeObservedPositions),
  );
}

function compareGroups(cohort, samples, keyForSample) {
  const groups = new Map();
  for (const sample of samples.filter((entry) =>
    COEFFICIENT_EQUIVALENT_HITS.includes(Number(entry.hit_event_id)))) {
    const key = keyForSample(sample);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(sample);
  }
  const comparable = [...groups.values()].filter(
    (group) => new Set(group.map((sample) => Number(sample.hit_event_id))).size > 1,
  );
  let equal = 0;
  let divergent = 0;
  let effectPresentEqual = 0;
  let effectPresentDivergent = 0;
  let divergentWithComponentIndexTransition = 0;
  let divergentWithIdenticalObservedGeometry = 0;
  const divergentExamples = [];
  for (const group of comparable) {
    const amounts = new Set(group.map((sample) => Number(sample.amount)));
    const effectPresent = statusState(cohort, group[0].source_status_state_id)
      .some((status) => Number(status.effect_id) === EFFECT_ID);
    if (amounts.size === 1) {
      equal += 1;
      if (effectPresent) effectPresentEqual += 1;
    } else {
      divergent += 1;
      if (effectPresent) effectPresentDivergent += 1;
      if (new Set(group.map((sample) => sample.packet.skill_effect_component_index)).size > 1) {
        divergentWithComponentIndexTransition += 1;
      }
      if (new Set(group.map((sample) => stableStringify(observedPositionCoordinates(sample)))).size === 1) {
        divergentWithIdenticalObservedGeometry += 1;
      }
      if (divergentExamples.length < 12) divergentExamples.push(compactGroup(group, effectPresent));
    }
  }
  return {
    summary: {
      comparable_groups: comparable.length,
      equal_output_groups: equal,
      divergent_output_groups: divergent,
      effect_present_equal_output_groups: effectPresentEqual,
      effect_present_divergent_output_groups: effectPresentDivergent,
      divergent_groups_with_component_index_transition:
        divergentWithComponentIndexTransition,
      divergent_groups_with_identical_last_observed_geometry:
        divergentWithIdenticalObservedGeometry,
    },
    divergent_examples: divergentExamples,
  };
}

function coefficientEquivalentKey(sample, includeObservedPositions) {
  const packet = sample.packet;
  return stableStringify([
    sample.session_id,
    sample.source_entity_uuid,
    sample.target_entity_uuid,
    sample.source_attribute_state_id,
    sample.target_attribute_state_id,
    sample.source_status_state_id,
    sample.target_status_state_id,
    sample.critical,
    sample.lucky,
    packet.owner_stage,
    packet.owner_level,
    packet.type_flags,
    packet.damage_mode,
    packet.position ?? null,
    includeObservedPositions ? observedPositionCoordinates(sample) : null,
  ]);
}

function strictRetainedInputKey(sample, includeObservedPositions) {
  const packet = sample.packet;
  return stableStringify([
    sample.session_id,
    sample.run_ordinal,
    sample.scene_id,
    sample.source_entity_uuid,
    sample.direct_source_entity_uuid,
    sample.target_entity_uuid,
    sample.hit_event_id,
    sample.source_actor_identity,
    sample.direct_source_actor_identity,
    sample.target_actor_identity,
    sample.source_attribute_state_id,
    sample.target_attribute_state_id,
    sample.source_status_state_id,
    sample.target_status_state_id,
    sample.status_provider_attribute_states,
    sample.critical,
    sample.lucky,
    sample.damage_source,
    sample.damage_type,
    packet.attacker_uuid,
    packet.top_summoner_uuid,
    packet.owner_id,
    packet.dead,
    packet.missed,
    packet.reported_critical,
    packet.type_flags,
    packet.owner_level,
    packet.owner_stage,
    packet.normal_hit,
    packet.property,
    packet.position,
    packet.damage_weight,
    packet.passive_uuid,
    packet.rainbow,
    packet.damage_mode,
    includeObservedPositions ? observedPositionCoordinates(sample) : null,
  ]);
}

function coefficientEquivalentKeyDescription(includeObservedPositions) {
  const fields = [
    "session_id", "source_entity_uuid", "target_entity_uuid",
    "source_attribute_state_id", "target_attribute_state_id",
    "source_status_state_id", "target_status_state_id", "critical", "lucky",
    "owner_stage", "owner_level", "type_flags", "damage_mode", "packet_position_x_y_z",
  ];
  if (includeObservedPositions) {
    fields.push("last_packet_observed_source_direct_source_target_coordinates");
  }
  return fields;
}

function strictRetainedInputKeyDescription(includeObservedPositions) {
  const fields = [
    "exact_numeric_hit_event_id",
    "session_run_scene_and_actor_identity",
    "source_direct_source_and_target_entity_uuids",
    "complete_retained_source_target_and_status_provider_attribute_states",
    "complete_retained_source_and_target_status_states",
    "damage_flags_and_non_output_packet_formula_inputs",
    "decoder_generated_skill_effect_component_index_and_count_excluded",
    "packet_position_x_y_z",
  ];
  if (includeObservedPositions) {
    fields.push("last_packet_observed_source_direct_source_target_coordinates");
  }
  return fields;
}

function observedPositionCoordinates(sample) {
  return [
    positionCoordinates(sample.source_position_at_wire_message_start),
    positionCoordinates(sample.direct_source_position_at_wire_message_start),
    positionCoordinates(sample.target_position_at_wire_message_start),
  ];
}

function positionCoordinates(position) {
  if (!position) return null;
  return [position.x, position.y, position.z, position.facing_radians];
}

function summarizePositionCoverage(samples) {
  const fields = [
    ["source", "source_position_at_wire_message_start"],
    ["direct_source", "direct_source_position_at_wire_message_start"],
    ["target", "target_position_at_wire_message_start"],
  ];
  const result = { selected_samples: samples.length };
  for (const [name, field] of fields) {
    const ages = samples
      .filter((sample) => sample[field] != null)
      .map((sample) => Number(sample.observed_micros) - Number(sample[field].observed_micros))
      .sort((left, right) => left - right);
    result[name] = {
      observed_samples: ages.length,
      missing_samples: samples.length - ages.length,
      observation_age_micros: quantiles(ages),
    };
  }
  result.all_three_observed_samples = samples.filter((sample) =>
    fields.every(([, field]) => sample[field] != null)).length;
  result.authority =
    "last packet-observed positions are provenance-bearing evidence, not proof of exact server-side event-time geometry";
  return result;
}

function quantiles(sortedValues) {
  if (sortedValues.length === 0) return null;
  const at = (fraction) =>
    sortedValues[Math.floor((sortedValues.length - 1) * fraction)];
  return {
    minimum: sortedValues[0],
    median: at(0.5),
    p95: at(0.95),
    maximum: sortedValues.at(-1),
  };
}

function compactGroup(group, effectPresent) {
  return group.slice(0, 8).map((sample) => ({
    session_id: sample.session_id,
    sequence: Number(sample.sequence),
    wire_capture_sequence: Number(sample.wire_capture_sequence),
    direct_source_entity_uuid: String(sample.direct_source_entity_uuid),
    target_entity_uuid: String(sample.target_entity_uuid),
    hit_event_id: Number(sample.hit_event_id),
    observed_damage: Number(sample.amount),
    owner_stage: sample.packet.owner_stage,
    owner_level: sample.packet.owner_level,
    skill_effect_group_index: sample.packet.skill_effect_group_index,
    skill_effect_component_index: sample.packet.skill_effect_component_index,
    skill_effect_component_count: sample.packet.skill_effect_component_count,
    normal_hit: sample.packet.normal_hit,
    packet_position: sample.packet.position,
    source_position_at_wire_message_start:
      sample.source_position_at_wire_message_start,
    direct_source_position_at_wire_message_start:
      sample.direct_source_position_at_wire_message_start,
    target_position_at_wire_message_start:
      sample.target_position_at_wire_message_start,
    effect_2110140_present: effectPresent,
  }));
}

function statusState(cohort, stateId) {
  return cohort.status_states[String(stateId)] ?? cohort.status_states[stateId] ?? [];
}

function verify(values) {
  const report = readJson(path.resolve(required(values, "input")));
  verifyReport(report);
  process.stdout.write(`${JSON.stringify(consoleSummary(report), null, 2)}\n`);
}

function verifyReport(report) {
  assert.equal(report.schema_version, SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.game_build, GAME_BUILD);
  assert.equal(report.identity.ability_id, ABILITY_ID);
  assert.equal(report.identity.support_effect_id, EFFECT_ID);
  assert.equal(report.policy.provider_rdps_credit_allowed, false);
  assert.equal(report.policy.runtime_promotion_allowed, false);
  assert.equal(report.static_route.exact_damage_attr_rows.length, 5);
  assert.equal(
    report.autoattack_operator_frontier.static_family_relation.autoattack_damage_attr_rows,
    265,
  );
  assert.equal(
    report.autoattack_operator_frontier.static_family_relation
      .formula_referenced_autoattack_rows,
    124,
  );
  assert.equal(
    report.autoattack_operator_frontier.static_family_relation
      .all_skill_description_referenced_autoattack_rows,
    131,
  );
  assert.equal(
    report.autoattack_operator_frontier.static_family_relation
      .coefficient_plus_fixed_relation_proven_for_description_linked_rows,
    true,
  );
  assert.deepEqual(
    report.autoattack_operator_frontier.static_family_relation
      .exact_ability_rows_with_direct_formula_text,
    [...EXPECTED_DAMAGE_ATTR_IDS.values()],
  );
  assert.equal(
    report.autoattack_operator_frontier.static_family_relation
      .own_skill_effect_formula_occurrences,
    0,
  );
  assert.equal(report.autoattack_operator_frontier.exact_stat_lane.exact_stat_lane_proven, true);
  assert.equal(report.autoattack_operator_frontier.exact_stat_lane.selected_attribute_id, 11_330);
  assert.deepEqual(
    Object.keys(report.autoattack_operator_frontier.exact_stat_lane.packet_damage_mode_counts),
    ["1"],
  );
  assert.equal(
    report.autoattack_operator_frontier.exact_stat_lane.packet_damage_mode_counts["1"],
    report.cohort.selected_samples,
  );
  assert.equal(
    report.autoattack_operator_frontier.client_formula_presentation_semantics
      .server_integer_rounding_proven,
    false,
  );
  assert.equal(
    report.autoattack_operator_frontier.client_formula_presentation_semantics
      .exact_format_function,
    "utility.number_tools.UnMarkAndPercentFormat",
  );
  assert.equal(
    report.autoattack_operator_frontier.exact_coefficient_selection
      .exact_coefficient_selection_proven,
    true,
  );
  assert.deepEqual(
    report.autoattack_operator_frontier.exact_coefficient_selection
      .selected_coefficient_basis_points_by_hit_event_id,
    { "3": 34_500, "5": 34_500, "7": 27_000, "8": 27_000, "9": 27_000 },
  );
  assert.equal(
    report.autoattack_operator_frontier.exact_coefficient_selection
      .packet_owner_stage_used_as_coefficient_index,
    false,
  );
  assert.equal(
    report.autoattack_operator_frontier.tier0_rounding_discriminant
      .exact_selected_actions,
    45,
  );
  assert.equal(
    report.autoattack_operator_frontier.tier0_rounding_discriminant
      .exact_same_packet_attribute_transition.current_physical_attack_delta,
    346,
  );
  assert.equal(
    report.autoattack_operator_frontier.tier0_rounding_discriminant
      .exact_same_packet_attribute_transition.internal_attack_add_delta,
    298,
  );
  assert.deepEqual(
    report.autoattack_operator_frontier.tier0_rounding_discriminant
      .projected_provider_damage_totals,
    {
      floor: 155_739,
      ceil: 155_786,
      nearest_half_up: 155_760,
      unrounded_rational: 155_738,
    },
  );
  assert.equal(
    report.autoattack_operator_frontier.tier0_rounding_discriminant
      .actions_with_floor_and_ceiling_projection_different,
    26,
  );
  assert.equal(
    report.autoattack_operator_frontier.tier0_rounding_discriminant
      .actions_with_all_four_candidate_projections_equal,
    0,
  );
  assert.equal(
    report.autoattack_operator_frontier.tier0_rounding_discriminant
      .exact_server_integer_rounding_proven,
    false,
  );
  assert.equal(
    report.autoattack_operator_frontier.tier0_rounding_discriminant
      .provider_rdps_credit_allowed,
    false,
  );
  assert.equal(
    report.autoattack_operator_frontier.same_wire_cross_coefficient_body_diagnostic
      .selected_wire_groups,
    2_096,
  );
  assert.equal(
    report.autoattack_operator_frontier.same_wire_cross_coefficient_body_diagnostic
      .cross_coefficient_body_wire_groups,
    1,
  );
  assert.equal(
    report.autoattack_operator_frontier.same_wire_cross_coefficient_body_diagnostic
      .cross_body_pairs_with_matching_critical_lucky_and_type_flags,
    0,
  );
  assert.equal(
    report.autoattack_operator_frontier.same_wire_cross_coefficient_body_diagnostic
      .shared_downstream_factor_proven,
    false,
  );
  assert.ok(report.cohort.source_effect_presence.present_samples > 0);
  assert.ok(report.cohort.source_effect_presence.absent_samples > 0);
  assert.equal(report.cohort.counterfactual_search.exact_controlled_groups, 0);
  assert.equal(
    report.autoattack_operator_frontier
      .coefficient_equivalent_position_diagnostic_groups
      .divergent_groups_with_component_index_transition,
    report.autoattack_operator_frontier
      .coefficient_equivalent_position_diagnostic_groups.divergent_output_groups,
  );
  assert.ok(Number.isInteger(
    report.autoattack_operator_frontier
      .exact_normalized_formula_input_and_position_groups.comparable_groups,
  ));
  assert.equal(report.conclusion.exact_autoattack_operator_proven, false);
  assert.equal(
    report.conclusion.shared_autoattack_coefficient_plus_fixed_relation_evidenced,
    true,
  );
  assert.equal(
    report.conclusion.exact_autoattack_row_coefficient_plus_fixed_relation_proven,
    true,
  );
  assert.equal(report.conclusion.exact_autoattack_stat_lane_proven, true);
  assert.equal(report.conclusion.promotion_decision, "fail_closed_no_provider_rdps_credit");
  assert.equal(report.content_sha256, contentHash(report));
}

function consoleSummary(report) {
  return {
    game_build: report.game_build,
    ability_id: report.identity.ability_id,
    selected_samples: report.cohort.selected_samples,
    source_effect_presence: report.cohort.source_effect_presence,
    strict_controlled_groups: report.cohort.counterfactual_search.exact_controlled_groups,
    static_family_relation: {
      autoattack_damage_attr_rows:
        report.autoattack_operator_frontier.static_family_relation.autoattack_damage_attr_rows,
      formula_referenced_autoattack_rows:
        report.autoattack_operator_frontier.static_family_relation
          .formula_referenced_autoattack_rows,
      exact_ability_rows_with_direct_formula_text:
        report.autoattack_operator_frontier.static_family_relation
          .exact_ability_rows_with_direct_formula_text,
    },
    exact_stat_lane: report.autoattack_operator_frontier.exact_stat_lane,
    tier0_rounding_discriminant: {
      exact_selected_actions:
        report.autoattack_operator_frontier.tier0_rounding_discriminant
          .exact_selected_actions,
      current_physical_attack_delta:
        report.autoattack_operator_frontier.tier0_rounding_discriminant
          .exact_same_packet_attribute_transition.current_physical_attack_delta,
      internal_attack_add_delta:
        report.autoattack_operator_frontier.tier0_rounding_discriminant
          .exact_same_packet_attribute_transition.internal_attack_add_delta,
      projected_provider_damage_totals:
        report.autoattack_operator_frontier.tier0_rounding_discriminant
          .projected_provider_damage_totals,
      floor_ceiling_different_actions:
        report.autoattack_operator_frontier.tier0_rounding_discriminant
          .actions_with_floor_and_ceiling_projection_different,
    },
    same_wire_cross_coefficient_body_diagnostic:
      report.autoattack_operator_frontier.same_wire_cross_coefficient_body_diagnostic,
    coefficient_equivalent_diagnostic_groups:
      report.autoattack_operator_frontier.coefficient_equivalent_diagnostic_groups,
    coefficient_equivalent_position_diagnostic_groups:
      report.autoattack_operator_frontier
        .coefficient_equivalent_position_diagnostic_groups,
    exact_normalized_formula_input_groups:
      report.autoattack_operator_frontier.exact_normalized_formula_input_groups,
    exact_normalized_formula_input_and_position_groups:
      report.autoattack_operator_frontier
        .exact_normalized_formula_input_and_position_groups,
    promotion_decision: report.conclusion.promotion_decision,
  };
}

function selfTest() {
  assert.deepEqual(
    autoattackFormulaMatches(
      '{*skillpara.damageMerge({129008400107},{1},"PVEDamageRadio","up")*} ATK/MATK +{*skillpara.damageMerge({129008400107},{1},"PVEFixedParameter","un")*}',
    ),
    [{
      coefficient_damage_attr_ids: [129_008_400_107],
      coefficient_hit_multipliers: [1],
      fixed_damage_attr_ids: [129_008_400_107],
      fixed_hit_multipliers: [1],
      arrays_align_exactly: true,
    }],
  );
  assert.deepEqual(numericTokens("x129008400107 y 2900840"), [129_008_400_107, 2_900_840]);
  assert.equal(
    autoattackStatLaneEvidence(
      [{ packet: { damage_mode: 1 } }],
      "public enum EDamageMode { DamageNormal = 0; DamagePhysical = 1; DamageMagical = 2; }",
    ).selected_attribute_id,
    11_330,
  );
  assert.equal(
    clientFormulaPresentationEvidence(
      'ret.numberFormFuncDic = { up = numberTools.UnMarkAndPercentFormat }\n' +
        'damageMerge = function() num = num + damageAttrTableRow[tableHeardName][level] * multiple; return ret.formatFloatParam(num, formatType) end',
      'function ret.UnMarkAndPercentFormat(value) local v = value / 100; local str = ret.removeTrailingZeros(v); return Lang("Percent", { val = str }) end',
    ).server_integer_rounding_proven,
    false,
  );
  assert.equal(roundPositiveRational(243_915_000, 10_000, "floor"), 24_391);
  assert.equal(roundPositiveRational(243_915_000, 10_000, "ceil"), 24_392);
  assert.equal(
    roundPositiveRational(243_915_000, 10_000, "nearest_half_up"),
    24_392,
  );
  assert.deepEqual(
    coefficientStageProjection({
      observedAmount: 117_566,
      currentAttack: 7_070,
      currentAttackDelta: 346,
      coefficientBasisPoints: 34_500,
      fixedParameter: 34,
      mode: "floor",
    }),
    {
      active_base: 24_425,
      inactive_base: 23_231,
      marginal: 1_194,
      marginal_key: "1194",
      projected_provider_damage: 5_747,
    },
  );
  assert.equal(
    coefficientStageProjection({
      observedAmount: 117_566,
      currentAttack: 7_070,
      currentAttackDelta: 346,
      coefficientBasisPoints: 34_500,
      fixedParameter: 34,
      mode: "unrounded_rational",
    }).projected_provider_damage,
    5_745,
  );

  const first = {
    session_id: "s", source_entity_uuid: 1, target_entity_uuid: 2,
    run_ordinal: 1, scene_id: 10, direct_source_entity_uuid: 3,
    source_actor_identity: null, direct_source_actor_identity: null,
    target_actor_identity: null,
    source_attribute_state_id: 3, target_attribute_state_id: 4,
    source_status_state_id: 5, target_status_state_id: 6,
    status_provider_attribute_states: [], critical: true, lucky: false,
    damage_source: null, damage_type: null, hit_event_id: 7, amount: 100,
    packet: { owner_stage: 1, owner_level: 1, type_flags: 1, damage_mode: 1,
      position: { x: 1, y: 2, z: 3 }, skill_effect_component_index: 1,
      skill_effect_component_count: 2 },
  };
  const second = structuredClone(first);
  second.hit_event_id = 8;
  second.amount = 110;
  second.packet.skill_effect_component_index = 2;
  assert.equal(
    coefficientEquivalentKey(first, false),
    coefficientEquivalentKey(second, false),
  );
  assert.notEqual(
    strictRetainedInputKey(first, false),
    strictRetainedInputKey(second, false),
  );
  second.hit_event_id = first.hit_event_id;
  assert.equal(
    strictRetainedInputKey(first, false),
    strictRetainedInputKey(second, false),
  );
  process.stdout.write("self-test passed\n");
}

function fileReceipt(filePath) {
  const stat = fs.statSync(filePath);
  return { path: filePath.replaceAll("\\", "/"), bytes: stat.size, sha256: sha256(filePath) };
}

function sha256(filePath) {
  const hash = crypto.createHash("sha256");
  const fd = fs.openSync(filePath, "r");
  const buffer = Buffer.allocUnsafe(1024 * 1024);
  try {
    for (;;) {
      const bytes = fs.readSync(fd, buffer, 0, buffer.length, null);
      if (bytes === 0) break;
      hash.update(buffer.subarray(0, bytes));
    }
  } finally {
    fs.closeSync(fd);
  }
  return hash.digest("hex");
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(stableStringify(copy)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function refuseExisting(filePath) {
  if (fs.existsSync(filePath)) throw new Error(`refusing to overwrite existing output: ${filePath}`);
}

function required(values, key) {
  const value = values.get(key);
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function parseArgs(args) {
  const values = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value == null) usage(1);
    values.set(key.slice(2), value);
  }
  return values;
}

function usage(exitCode) {
  process.stderr.write(
    "usage:\n" +
    "  node tools/bpsr-autoattack-single-skill-trace.mjs generate --cohort FILE --counterfactual FILE --static-worklist FILE --inheritance-proof FILE --decoder-source FILE --damage-attr FILE --skill-effect FILE --skill-fight-level FILE --skill-aoyi FILE --il2cpp-dump FILE --skill-vm-bytecode FILE --skill-vm-decompiled FILE --number-tools-bytecode FILE --number-tools-decompiled FILE --enum-define-bytecode FILE --enum-define-decompiled FILE --primary-attack-transition-proof FILE --family-transition-search FILE --output FILE\n" +
    "  node tools/bpsr-autoattack-single-skill-trace.mjs verify --input FILE\n" +
    "  node tools/bpsr-autoattack-single-skill-trace.mjs self-test\n",
  );
  process.exit(exitCode);
}
