#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-controlled-55228-baseline-component-proof.mjs";
const BUILD = "24687926";
const EFFECT_ID = 55_228;
const ACTION_ID = 2_031_102;
const TARGET_MONSTER_IDS = [33_527, 33_529, 33_530];
const COMPONENT_IDS = [25_204, 683_115, 2_032_274, 2_110_111, 2_203_031,
  2_300_621, 3_003_012, 3_003_014];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(parsed) {
  const files = {
    effect_counterfactual: resolved(parsed, "effect-counterfactual"),
    component_counterfactual: resolved(parsed, "component-counterfactual"),
    inverse_proof: resolved(parsed, "inverse-proof"),
    current_hp_route_proof: resolved(parsed, "current-hp-route-proof"),
    buff_table: resolved(parsed, "buff-table"),
    monster_table: resolved(parsed, "monster-table"),
    formula_runtime: resolved(parsed, "formula-runtime"),
    value_runtime: resolved(parsed, "value-runtime"),
    il2cpp_dump: resolved(parsed, "il2cpp-dump"),
  };
  const output = path.resolve(required(parsed, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);
  const documents = Object.fromEntries(Object.entries(files).map(([key, file]) =>
    [key, key === "il2cpp_dump" ? readFileSync(file, "utf8") : readJson(file, key)]));
  const inputs = Object.fromEntries(Object.entries(files).map(([key, file]) =>
    [key, descriptor(file)]));
  const report = buildReport(documents, inputs);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote ${output}`);
}

function buildReport(documents, inputs) {
  const effectProof = documents.effect_counterfactual;
  assert(String(effectProof?.game_build) === BUILD && effectProof?.schema_version === 19,
    "Effect counterfactual identity mismatch");
  const effect = only(effectProof.effects, "effect counterfactual");
  assert(effect.effect_id === EFFECT_ID && effect.locus === "target",
    "Effect 55228 target counterfactual is missing");
  const examples = effect.target_current_hp_excluded_diagnostic?.divergent_examples
    ?.filter((row) => row.ability_id === ACTION_ID) ?? [];
  assert(examples.length === 4, "Expected four exact action 2031102 controlled groups");
  const critical = examples.filter((row) => row.present_outcome.amount === 272_418 &&
    row.absent_outcome.amount === 258_416);
  const noncritical = examples.filter((row) => row.present_outcome.amount === 114_422 &&
    row.absent_outcome.amount === 108_540);
  assert(critical.length === 3 && noncritical.length === 1,
    "Controlled output strata changed");
  for (const row of examples) validateControlledEffectExample(row);

  const componentProof = documents.component_counterfactual;
  assert(String(componentProof?.game_build) === BUILD && componentProof?.schema_version === 19,
    "Component counterfactual identity mismatch");
  assertExact(componentProof.processing?.selected_effect_ids, COMPONENT_IDS,
    "Component scan effect IDs");
  assert(componentProof.processing?.scanned_samples === 56_083 &&
    componentProof.processing?.measured_peak_within_configured_limit === true,
  "Component scan did not retain its bounded-memory proof");
  const componentRows = COMPONENT_IDS.map((id) => summarizeComponent(componentProof, id));
  const module = componentRows.find((row) => row.effect_id === 2_300_621);
  assert(module.controlled_groups === 3 && module.divergent_groups === 2 &&
    module.equal_groups === 1, "Module 2300621 controlled frontier changed");
  const externalModule = only(module.divergent_examples.filter((row) =>
    row.provider_relationship === "third_party"), "third-party module example");
  assert(externalModule.status.source_entity_uuid === 190_072_160_896 &&
    externalModule.status.stacks === 2 && externalModule.observed_delta === 15_844,
  "Third-party module example identity mismatch");

  const monsters = TARGET_MONSTER_IDS.map((id) => documents.monster_table?.[String(id)]);
  assert(monsters.every((monster, index) => Number(monster?.Id) === TARGET_MONSTER_IDS[index] &&
    Number(monster?.MonsterType) === 0), "Controlled target MonsterTable rows mismatch");
  const enumMatch = documents.il2cpp_dump.match(
    /public enum EMonsterType[\s\S]{0,500}?Monster = 0;[\s\S]{0,120}?Elite = 1;[\s\S]{0,120}?Boss = 2;/,
  );
  assert(enumMatch, "Current-build EMonsterType enum mapping was not found");
  const cuisine = documents.buff_table?.["2032274"];
  assert(Number(cuisine?.Id) === 2_032_274 && /Elites or stronger enemies \+10%/.test(cuisine.Desc),
    "Cuisine current-build conditional clause mismatch");

  const inverse = documents.inverse_proof;
  assert(inverse?.schema_version === 1 && inverse?.effect_id === EFFECT_ID &&
    inverse?.adjudication?.exact_current_build_magnitude_proven === false,
  "Inverse proof identity or fail-closed state mismatch");
  const alternatives = only(inverse.inverse_results_by_capture, "inverse capture")
    .alternative_delta_demonstration;
  assertExact(alternatives.map((row) => row.delta_basis_points), [750, 1000, 1250],
    "Inverse scalar alternatives");

  const hpRoute = documents.current_hp_route_proof;
  assert(hpRoute?.schema_version === 1 && hpRoute?.effect_id === EFFECT_ID &&
    hpRoute?.summary?.selected_outgoing_health_dependent_catalog_routes_remaining === 0 &&
    hpRoute?.summary?.exact_selected_action_target_current_hp_independence_proven === false &&
    hpRoute?.summary?.exact_selected_action_target_current_hp_dependency_proven === false,
  "Current-HP route proof identity mismatch");

  const formulaRuntime = documents.formula_runtime;
  const valueRuntime = documents.value_runtime;
  assert(formulaRuntime?.schemaVersion === 1 && valueRuntime?.schemaVersion === 1,
    "Modifier runtime identity mismatch");
  const runtimeRows = COMPONENT_IDS.map((id) => ({
    effect_id: id,
    formula: compactFormula(formulaRuntime.entriesByKey?.[`buffs:${id}`]),
    value: compactValue(valueRuntime.entriesByKey?.[`buffs:${id}`]),
  }));

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: BUILD,
    effect_id: EFFECT_ID,
    action_id: ACTION_ID,
    inputs,
    policy: {
      exact_numeric_ids_and_build_identity_authoritative: true,
      localized_names_are_semantic_evidence_only: true,
      target_current_hp_is_not_silently_ignored: true,
      remote_profile_state_is_not_reconstructed_from_local_snapshots: true,
      observed_cooccurrence_is_not_formula_or_causality_authority: true,
      unresolved_components_are_preserved: true,
      provider_rdps_credit_fail_closed: true,
    },
    controlled_effect_55228: {
      controlled_groups: examples.length,
      sample_comparisons: effect.target_current_hp_excluded_diagnostic.sample_comparisons,
      provider_relationship: "third_party",
      critical_stratum: summarizeStratum(critical),
      noncritical_stratum: summarizeStratum(noncritical),
      source_attribute_state_ids: unique(examples.map((row) =>
        row.present_formula_context.source_attribute_state_id)),
      source_status_state_ids: unique(examples.map((row) =>
        row.present_formula_context.source_status_state_id)),
      exact_observed_delta_proven: true,
      exact_current_build_scalar_proven: false,
    },
    target_identity: {
      monster_ids: TARGET_MONSTER_IDS,
      rows: monsters.map((monster) => ({
        monster_id: monster.Id,
        name_evidence: monster.Name,
        monster_type_raw: monster.MonsterType,
      })),
      current_build_enum: { normal_monster: 0, elite: 1, boss: 2 },
      elite_or_stronger_predicate_satisfied: false,
      cuisine_2032274_generic_damage_10_percent_applies: false,
      cuisine_flat_matk_is_common_source_context_not_a_target_predicate_result: true,
    },
    component_counterfactual_frontier: {
      scanned_samples: componentProof.processing.scanned_samples,
      measured_peak_working_set_mib: componentProof.processing.measured_peak_working_set_mib,
      memory_limit_mib: componentProof.processing.memory_limit_mib,
      relaxed_controlled_groups: componentProof.summary.relaxed_controlled_groups,
      relaxed_divergent_output_groups: componentProof.summary.relaxed_divergent_output_groups,
      components: componentRows,
      exact_runtime_routes: runtimeRows,
    },
    external_module_2300621_adjudication: {
      target_status_provider_entity_uuid: externalModule.status.source_entity_uuid,
      attributed_damage_source_entity_uuid: externalModule.source_entity_uuid,
      provider_relationship: externalModule.provider_relationship,
      stacks: externalModule.status.stacks,
      observed_present_amount: externalModule.present_amount,
      observed_absent_amount: externalModule.absent_amount,
      observed_delta: externalModule.observed_delta,
      source_attributes_equal: externalModule.source_attributes_equal,
      source_statuses_equal: externalModule.source_statuses_equal,
      target_status_difference_only_selected_instance: externalModule.target_status_difference_only_selected_instance,
      target_attribute_difference_only_current_hp: externalModule.target_attribute_difference_only_current_hp,
      owner_scoped_description_conflicts_with_naive_external_transfer_interpretation: true,
      provider_profile_module_level_proven: false,
      target_current_hp_or_hidden_server_state_excluded_as_cause: false,
      external_rdps_transfer_proven: false,
    },
    current_hp_adjudication: {
      target_side_catalog_routes_remaining: 0,
      critical_output_is_status_stratified_invariant_across_three_targets: true,
      critical_present_current_hp_range: hpRange(critical, "present_formula_context"),
      critical_absent_current_hp_range: hpRange(critical, "absent_formula_context"),
      intrinsic_server_action_target_hp_behavior_globally_excluded: false,
      reason: "The exact catalog has no outgoing target-HP route and three critical groups repeat the same active/absent amounts across different targets and HP values, but a generic server-side action rule or hidden state is not yet excluded.",
    },
    scalar_frontier: {
      tested_delta_basis_points: alternatives.map((row) => ({
        delta_basis_points: row.delta_basis_points,
        compatible_total_factor_basis_points: row.compatible_total_factor_basis_points,
      })),
      cuisine_conditional_1000_removed_from_controlled_target_baseline: true,
      exact_scalar_proven: false,
      operation_order_proven: false,
      integer_rounding_proven: false,
      formula_specific_conservation_proven: false,
    },
    smallest_safe_next_slice: [
      "obtain or derive the encounter-time Frost Mage X10 grade for remote attacker 2474661 without substituting a local profile",
      "prove whether target status 2300621 is server-filtered to its provider or is a genuinely external target-wide modifier",
      "close intrinsic target-current-HP or hidden-state behavior for action 2031102 before treating HP-excluded pairs as causal",
      "replay the resulting exact counterfactual through integer conservation before enabling provider credit",
    ],
    promotion_counts: {
      production_runtime_ui: 0,
      superseded_historical_experiments: 2,
      active_current_build_candidates: 1,
    },
    summary: {
      cuisine_elite_damage_clause_excluded_for_controlled_target: true,
      bounded_component_scan_complete: true,
      third_party_module_transition_found: true,
      third_party_module_transition_promotable: false,
      current_build_effect_55228_scalar_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      unresolved_evidence_hidden: false,
    },
  };
}

function validateControlledEffectExample(row) {
  assert(row.status?.effect_id === EFFECT_ID && row.provider_relationship === "third_party",
    "Controlled effect provider relationship changed");
  assert(row.source_entity_uuid === 162_179_383_936 && row.status.source_entity_uuid === 492_224,
    "Controlled effect provider/source identity changed");
  const present = row.present_formula_context;
  const absent = row.absent_formula_context;
  assert(present.source_attribute_state_id === absent.source_attribute_state_id &&
    present.source_status_state_id === absent.source_status_state_id,
  "Controlled source state changed");
  const targetDiff = attributeDiff(present.target_attributes, absent.target_attributes);
  assert(targetDiff.length === 1 && targetDiff[0].attribute_id === 11_310,
    "Controlled target changed outside Current HP");
  const presentMonster = attributeValue(present.target_attributes, 10);
  const absentMonster = attributeValue(absent.target_attributes, 10);
  assert(presentMonster === absentMonster && TARGET_MONSTER_IDS.includes(presentMonster),
  "Controlled target monster identity changed");
  const statusDiff = statusDifference(present.target_statuses, absent.target_statuses);
  assert(statusDiff.only_present.length === 1 && statusDiff.only_absent.length === 0 &&
    statusDiff.only_present[0].effect_id === EFFECT_ID,
  "Controlled target status changed outside effect 55228");
}

function summarizeComponent(document, effectId) {
  const effects = document.effects.filter((row) => row.effect_id === effectId);
  assert(effects.length > 0, `Missing component effect ${effectId}`);
  let controlled = 0;
  let divergent = 0;
  let equal = 0;
  const examples = [];
  for (const effect of effects) {
    for (const variant of effect.variants) {
      const mode = variant.target_current_hp_excluded_diagnostic;
      controlled += mode?.controlled_groups ?? 0;
      divergent += mode?.divergent_output_groups ?? 0;
      equal += mode?.equal_output_groups ?? 0;
      for (const example of mode?.divergent_examples ?? []) {
        examples.push(compactExample(example));
      }
    }
  }
  return {
    effect_id: effectId,
    loci: unique(effects.map((row) => row.locus)),
    controlled_groups: controlled,
    divergent_groups: divergent,
    equal_groups: equal,
    divergent_examples: examples,
    formula_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function compactExample(row) {
  const present = row.present_formula_context;
  const absent = row.absent_formula_context;
  const targetAttrDiff = attributeDiff(present.target_attributes, absent.target_attributes);
  const targetStatusDiff = statusDifference(present.target_statuses, absent.target_statuses);
  return {
    source_entity_uuid: row.source_entity_uuid,
    target_entity_uuid: row.target_entity_uuid,
    ability_id: row.ability_id,
    status: row.status,
    provider_relationship: row.provider_relationship,
    present_amount: row.present_outcome.amount,
    absent_amount: row.absent_outcome.amount,
    observed_delta: row.present_outcome.amount - row.absent_outcome.amount,
    present_sequences: row.present_sequences,
    absent_sequences: row.absent_sequences,
    source_attributes_equal: present.source_attribute_state_id === absent.source_attribute_state_id,
    source_statuses_equal: present.source_status_state_id === absent.source_status_state_id,
    target_attribute_differences: targetAttrDiff,
    target_attribute_difference_only_current_hp: targetAttrDiff.length === 1 &&
      targetAttrDiff[0].attribute_id === 11_310,
    target_status_difference: targetStatusDiff,
    target_status_difference_only_selected_instance: targetStatusDiff.only_present.length === 1 &&
      targetStatusDiff.only_absent.length === 0 &&
      targetStatusDiff.only_present[0].effect_id === row.status.effect_id,
  };
}

function summarizeStratum(rows) {
  return {
    groups: rows.length,
    target_entity_uuids: unique(rows.map((row) => row.target_entity_uuid)),
    present_amounts: unique(rows.map((row) => row.present_outcome.amount)),
    absent_amounts: unique(rows.map((row) => row.absent_outcome.amount)),
    observed_deltas: unique(rows.map((row) =>
      row.present_outcome.amount - row.absent_outcome.amount)),
  };
}

function hpRange(rows, contextKey) {
  const values = rows.map((row) => attributeValue(row[contextKey].target_attributes, 11_310));
  return { minimum: Math.min(...values), maximum: Math.max(...values), distinct_values: unique(values) };
}

function compactFormula(row) {
  if (!row) return null;
  return {
    formula_readiness: row.formulaReadiness ?? null,
    formula_zone_ids: row.formulaZoneIds ?? [],
    value_resolution: row.valueResolution ?? null,
    scope_kinds: row.scopeKinds ?? [],
    stack_policy: row.stackPolicy ?? null,
  };
}

function compactValue(row) {
  if (!row) return null;
  return {
    value_proof_status: row.valueProofStatus ?? null,
    selected_values: row.selectedValues ?? [],
    value_blockers: row.valueBlockers ?? [],
  };
}

function attributeValue(attributes, id) {
  return attributes.find((row) => row.attribute_id === id)?.value ?? null;
}

function attributeDiff(present, absent) {
  const left = new Map(present.map((row) => [row.attribute_id, row.value]));
  const right = new Map(absent.map((row) => [row.attribute_id, row.value]));
  return unique([...left.keys(), ...right.keys()]).filter((id) => left.get(id) !== right.get(id))
    .map((attributeId) => ({
      attribute_id: attributeId,
      present_value: left.get(attributeId) ?? null,
      absent_value: right.get(attributeId) ?? null,
    }));
}

function statusDifference(present, absent) {
  const key = (row) => [row.effect_id, row.source_entity_uuid, row.stacks, row.level,
    row.origin_source_type_id, row.origin_source_config_id].join("|");
  const left = new Map(present.map((row) => [key(row), row]));
  const right = new Map(absent.map((row) => [key(row), row]));
  return {
    only_present: [...left].filter(([id]) => !right.has(id)).map(([, row]) => row),
    only_absent: [...right].filter(([id]) => !left.has(id)).map(([, row]) => row),
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
  assert(report?.game_build === BUILD && report?.effect_id === EFFECT_ID &&
    report?.action_id === ACTION_ID, "Report build/effect/action mismatch");
  assert(report?.controlled_effect_55228?.controlled_groups === 4 &&
    report?.controlled_effect_55228?.critical_stratum?.groups === 3 &&
    report?.controlled_effect_55228?.noncritical_stratum?.groups === 1,
  "Controlled effect census mismatch");
  assertExact(report?.target_identity?.monster_ids, TARGET_MONSTER_IDS,
    "Target monster IDs");
  assert(
    report?.target_identity?.elite_or_stronger_predicate_satisfied === false &&
    report?.target_identity?.cuisine_2032274_generic_damage_10_percent_applies === false,
  "Target predicate adjudication mismatch");
  assert(report?.external_module_2300621_adjudication?.provider_relationship === "third_party" &&
    report?.external_module_2300621_adjudication?.external_rdps_transfer_proven === false,
  "External module fail-closed adjudication mismatch");
  assert(report?.summary?.bounded_component_scan_complete === true &&
    report?.summary?.current_build_effect_55228_scalar_proven === false &&
    report?.summary?.provider_rdps_credit_allowed === false &&
    report?.summary?.runtime_promotion_allowed === false &&
    report?.promotion_counts?.production_runtime_ui === 0,
  "Report granted unsafe promotion authority");
  if (report.content_sha256 !== undefined) {
    assert(report.content_sha256 === contentHash(report), "Content hash mismatch");
  }
}

function selfTest() {
  const present = [{ effect_id: 7, source_entity_uuid: 3, stacks: 1, level: 1,
    origin_source_type_id: 1, origin_source_config_id: 2 }];
  const diff = statusDifference(present, []);
  assert(diff.only_present.length === 1 && diff.only_absent.length === 0,
    "Status difference self-test failed");
  assertExact(unique([3, 1, 3, 2]), [3, 1, 2], "Unique self-test");
  console.log("bpsr-controlled-55228-baseline-component-proof self-test passed");
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

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`Unable to read ${label} ${file}: ${error.message}`);
  }
}

function descriptor(file) {
  return {
    path: file.replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: `sha256:${createHash("sha256").update(readFileSync(file)).digest("hex")}`,
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

function only(values, label) {
  assert(Array.isArray(values) && values.length === 1, `Expected one ${label}`);
  return values[0];
}

function assertExact(actual, expected, label) {
  assert(JSON.stringify(actual) === JSON.stringify(expected), `${label} mismatch`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(code) {
  console.log("Usage:\n  node tools/bpsr-controlled-55228-baseline-component-proof.mjs build --effect-counterfactual <json> --component-counterfactual <json> --inverse-proof <json> --current-hp-route-proof <json> --buff-table <json> --monster-table <json> --formula-runtime <json> --value-runtime <json> --il2cpp-dump <dump.cs> --output <json>\n  node tools/bpsr-controlled-55228-baseline-component-proof.mjs verify --input <json>\n  node tools/bpsr-controlled-55228-baseline-component-proof.mjs self-test");
  process.exit(code);
}
