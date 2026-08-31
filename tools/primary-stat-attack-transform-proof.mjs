#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const paths = Object.fromEntries([
  "professionTable",
  "talentTable",
  "talentStageTable",
  "fightAttrTable",
  "attrDescription",
].map((key) => [key, resolvePath(options[key])]));
const outputPath = resolvePath(options.output);

const professions = readJson(paths.professionTable, "ProfessionSystemTable");
const talents = readJson(paths.talentTable, "TalentTable");
const talentStages = readJson(paths.talentStageTable, "TalentStageTable");
const fightAttrs = readJson(paths.fightAttrTable, "FightAttrTable");
const attrDescriptions = readJson(paths.attrDescription, "AttrDescription");

const familyContracts = new Map([
  [11010, {
    primary_name: "Strength",
    attack_attribute_id: 11330,
    attack_add_attribute_id: 11332,
    talent_effect_source_selector: 0,
    coefficient_basis_points: 1250,
    source_units: 8,
    attack_units: 1,
    witness_talent_ids: [300, 401],
    witness_description_ids: [2205020, 2208010],
  }],
  [11020, {
    primary_name: "Intellect",
    attack_attribute_id: 11340,
    attack_add_attribute_id: 11342,
    talent_effect_source_selector: 3,
    coefficient_basis_points: 1000,
    source_units: 10,
    attack_units: 1,
    witness_talent_ids: [201, 501, 1303],
    witness_description_ids: [2202030, 2204020, 2207040],
  }],
  [11030, {
    primary_name: "Agility",
    attack_attribute_id: 11330,
    attack_add_attribute_id: 11332,
    talent_effect_source_selector: 2,
    coefficient_basis_points: 1250,
    source_units: 8,
    attack_units: 1,
    witness_talent_ids: [101],
    witness_description_ids: [2200030],
  }],
]);

const expectedActiveClasses = new Map([
  [1, ["Stormblade", 11030, 11330, [101, 102]]],
  [2, ["Frost Mage", 11020, 11340, [104, 105]]],
  [3, ["Twin Striker", 11010, 11330, [128, 129]]],
  [4, ["Wind Knight", 11010, 11330, [107, 108]]],
  [5, ["Verdant Oracle", 11020, 11340, [110, 111]]],
  [9, ["Heavy Guardian", 11010, 11330, [113, 114]]],
  [11, ["Marksman", 11030, 11330, [116, 117]]],
  [12, ["Shield Knight", 11010, 11330, [122, 123]]],
  [13, ["Beat Performer", 11020, 11340, [119, 120]]],
]);

const activeProfessions = Object.values(professions)
  .filter((row) => Number(row.Create) === 1 && row.IsOpen === true)
  .sort((left, right) => Number(left.Id) - Number(right.Id));
assert(activeProfessions.length === expectedActiveClasses.size,
  `expected ${expectedActiveClasses.size} active classes, found ${activeProfessions.length}`);

const classRoutes = [];
for (const profession of activeProfessions) {
  const classId = Number(profession.Id);
  const expected = expectedActiveClasses.get(classId);
  assert(expected, `active class ${classId} is not in the exact-build contract`);
  const [expectedName, expectedPrimaryId, expectedAttackId, expectedStageIds] = expected;
  assert(String(profession.Name) === expectedName,
    `class ${classId} name changed from ${expectedName} to ${profession.Name}`);
  const primarySelections = normalizeModeSelections(profession.StrOrIntOrDexShow);
  const attackSelections = normalizeModeSelections(profession.AttackShow);
  assert(primarySelections.length === 2 && primarySelections.every((row) => row.attribute_id === expectedPrimaryId),
    `class ${classId} primary-stat selection changed`);
  assert(attackSelections.length === 2 && attackSelections.every((row) => row.attribute_id === expectedAttackId),
    `class ${classId} attack selection changed`);
  const stageIds = numbers(profession.ShowTalentStage);
  assert(equalArrays(stageIds, expectedStageIds), `class ${classId} spec stages changed`);
  for (const stageId of stageIds) {
    assert(talentStages[String(stageId)], `class ${classId} talent stage ${stageId} is missing`);
  }
  const family = familyContracts.get(expectedPrimaryId);
  assert(family?.attack_attribute_id === expectedAttackId,
    `class ${classId} primary family does not select attack ${expectedAttackId}`);
  classRoutes.push({
    class_id: classId,
    class_name: expectedName,
    primary_attribute_id: expectedPrimaryId,
    primary_attribute_name: family.primary_name,
    attack_attribute_id: expectedAttackId,
    attack_attribute_name: expectedAttackId === 11340 ? "MATK" : "ATK",
    attack_add_attribute_id: family.attack_add_attribute_id,
    mode_selections: {
      primary: primarySelections,
      attack: attackSelections,
    },
    spec_stage_ids: stageIds,
    transform_family_id: `${expectedPrimaryId}->${family.attack_add_attribute_id}`,
  });
}

const familyRows = [];
for (const [primaryAttributeId, contract] of familyContracts) {
  const primaryAttr = requireRow(fightAttrs, primaryAttributeId, "FightAttrTable");
  const attackAttr = requireRow(fightAttrs, contract.attack_attribute_id, "FightAttrTable");
  assert(String(primaryAttr.OfficialName) === contract.primary_name,
    `attribute ${primaryAttributeId} official name changed`);
  assert(Number(attackAttr.AttrAdd) === contract.attack_add_attribute_id,
    `attack ${contract.attack_attribute_id} add member changed`);
  assert(String(primaryAttr.Type) === "int32" && String(attackAttr.Type) === "int32",
    `transform ${primaryAttributeId} is no longer an integer attribute route`);

  const talentWitnesses = contract.witness_talent_ids.map((talentId) => {
    const talent = requireRow(talents, talentId, "TalentTable");
    const effect = (talent.TalentEffect || []).find((candidate) =>
      Number(candidate?.[0]) === 4
      && Number(candidate?.[1]) === contract.talent_effect_source_selector
      && Number(candidate?.[2]) === contract.attack_add_attribute_id);
    assert(effect, `talent ${talentId} no longer contains the primary-to-attack opcode`);
    assert(Number(effect[3]) === contract.coefficient_basis_points,
      `talent ${talentId} transform coefficient changed`);
    assert(descriptionHasRatio(talent.TalentDes, contract.source_units, contract.attack_units),
      `talent ${talentId} description no longer proves ${contract.source_units}:${contract.attack_units}`);
    return {
      talent_id: talentId,
      weapon_group: Number(talent.WeaponGroup),
      name: String(talent.TalentName),
      raw_effect: effect.map(Number),
      description: stripMarkup(talent.TalentDes),
    };
  });

  const descriptionWitnesses = contract.witness_description_ids.map((descriptionId) => {
    const row = requireRow(attrDescriptions, descriptionId, "AttrDescription");
    assert(descriptionHasRatio(row.Description, contract.source_units, contract.attack_units),
      `attribute description ${descriptionId} no longer proves ${contract.source_units}:${contract.attack_units}`);
    assert(stripMarkup(row.Description).includes(contract.primary_name),
      `attribute description ${descriptionId} no longer names ${contract.primary_name}`);
    return { description_id: descriptionId, description: stripMarkup(row.Description) };
  });

  const recommendedClassIds = numbers(primaryAttr.RecomProfessionId)
    .filter((classId) => expectedActiveClasses.has(classId));
  const selectedClassIds = classRoutes
    .filter((row) => row.primary_attribute_id === primaryAttributeId)
    .map((row) => row.class_id);
  assert(equalSets(recommendedClassIds, selectedClassIds),
    `${contract.primary_name} recommended active classes differ from ProfessionSystem selection`);

  familyRows.push({
    transform_family_id: `${primaryAttributeId}->${contract.attack_add_attribute_id}`,
    primary_attribute_id: primaryAttributeId,
    primary_attribute_name: contract.primary_name,
    attack_attribute_id: contract.attack_attribute_id,
    attack_add_attribute_id: contract.attack_add_attribute_id,
    talent_effect_source_selector: contract.talent_effect_source_selector,
    coefficient_basis_points: contract.coefficient_basis_points,
    fixed_point_denominator: 10000,
    exact_ratio: `${contract.attack_units}/${contract.source_units}`,
    formula: `trunc_nonnegative(primary_total * ${contract.coefficient_basis_points} / 10000)`,
    rounding: "integer truncation toward zero; primary stats are nonnegative, therefore floor",
    selected_active_class_ids: selectedClassIds,
    fight_attr_recommended_active_class_ids: recommendedClassIds,
    talent_opcode_witnesses: talentWitnesses,
    localized_ratio_witnesses: descriptionWitnesses,
  });
}

const specialConversions = [
  [901, 9, 11010, 11352, 12000, "Strength-to-Armor"],
  [1101, 11, 11030, 11122, 7000, "Agility-to-Haste"],
  [1206, 12, 11010, 11352, 12000, "Strength-to-Armor"],
].map(([talentId, classId, primaryId, targetId, coefficient, semantic]) => {
  const talent = requireRow(talents, talentId, "TalentTable");
  const effect = (talent.TalentEffect || []).find((candidate) =>
    Number(candidate?.[0]) === 4 && Number(candidate?.[2]) === targetId);
  assert(effect && Number(effect[3]) === coefficient,
    `special conversion talent ${talentId} changed`);
  assert(Number(talent.WeaponGroup) === classId,
    `special conversion talent ${talentId} changed class ownership`);
  return {
    talent_id: talentId,
    class_id: classId,
    primary_attribute_id: primaryId,
    target_attribute_id: targetId,
    coefficient_basis_points: coefficient,
    semantic,
    raw_effect: effect.map(Number),
    description: stripMarkup(talent.TalentDes),
    relationship_to_base_attack_transform: "additional class-tree conversion; does not replace ProfessionSystem primary/attack selection",
  };
});

const result = {
  schema_version: 1,
  generated_by: "tools/primary-stat-attack-transform-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: String(options.gameBuild),
  proof_state: "offline-primary-stat-attack-transform-complete",
  policy: {
    exact_build_required: true,
    descriptions_alone_are_formula_authority: false,
    formula_requires_structural_opcode_and_description_agreement: true,
    every_active_class_and_spec_is_fail_closed: true,
    inactive_or_unreleased_classes_are_not_runtime_claims: true,
    special_class_tree_conversions_do_not_replace_base_attack_conversion: true,
    unresolved_evidence_is_hidden: false,
    future_active_class_or_changed_route_reopens_gate: true,
  },
  inputs: Object.fromEntries(Object.entries(paths).map(([key, value]) => [key, relative(value)])),
  summary: {
    active_classes_proven: classRoutes.length,
    active_specs_proven: classRoutes.reduce((sum, row) => sum + row.spec_stage_ids.length, 0),
    primary_transform_families_proven: familyRows.length,
    structural_talent_witnesses: familyRows.reduce((sum, row) => sum + row.talent_opcode_witnesses.length, 0),
    localized_ratio_witnesses: familyRows.reduce((sum, row) => sum + row.localized_ratio_witnesses.length, 0),
    special_non_attack_conversions_separated: specialConversions.length,
    remaining_supported_class_routes: 0,
  },
  transform_contract: {
    input: "ProfessionSystem-selected final primary attribute for the encounter-local class",
    output: "selected ATK/MATK additive family member before the normal final-attribute fold",
    fixed_point_denominator: 10000,
    rounding: "truncation toward zero into the int32 target member",
    downstream_fold: "packet-attribute-family-transform",
  },
  families: familyRows,
  active_class_routes: classRoutes,
  special_class_tree_conversions: specialConversions,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function normalizeModeSelections(value) {
  return (value || []).map((row) => ({ mode: Number(row?.[0]), attribute_id: Number(row?.[1]) }));
}

function descriptionHasRatio(value, sourceUnits, attackUnits) {
  const text = stripMarkup(value);
  return new RegExp(`\\b${sourceUnits}\\b`).test(text) && new RegExp(`\\b${attackUnits}\\b`).test(text);
}

function stripMarkup(value) {
  return String(value || "").replace(/<[^>]+>/g, "").replace(/\s+/g, " ").trim();
}

function numbers(value) {
  return (value || []).map(Number).filter(Number.isSafeInteger);
}

function equalArrays(left, right) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function equalSets(left, right) {
  return equalArrays([...new Set(left)].sort((a, b) => a - b), [...new Set(right)].sort((a, b) => a - b));
}

function requireRow(table, id, label) {
  const row = table[String(id)];
  assert(row, `${label} row ${id} is missing`);
  return row;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    parsed[key] = value;
  }
  for (const required of [
    "gameBuild", "professionTable", "talentTable", "talentStageTable",
    "fightAttrTable", "attrDescription", "output",
  ]) {
    if (!parsed[required]) throw new Error(`missing --${required}`);
  }
  return parsed;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`failed to read ${label} at ${filePath}: ${error.message}`);
  }
}

function relative(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}
