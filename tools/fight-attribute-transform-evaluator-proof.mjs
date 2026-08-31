#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const auditPath = resolvePath(options.audit);
const tablePath = resolvePath(options.table);
const fightAttrTablePath = resolvePath(options.fightAttrTable);
const outputPath = resolvePath(options.output);

const audit = readJson(auditPath, "Lua 5.3 attribute-transform consumer audit");
const table = readJson(tablePath, "FightAttrTranTable");
const fightAttrTable = readJson(fightAttrTablePath, "FightAttrTable");

const expectedAttributeMappings = [
  { target: "Crit", transform: "CriToCrit", raw: "Cri" },
  { target: "HastePct", transform: "HasteToHastePct", raw: "Haste" },
  { target: "LuckyStrikeProb", transform: "LuckToLuckyStrikeProb", raw: "Luck" },
  { target: "VersatilityPct", transform: "VersatilityToVersatilityPct", raw: "Versatility" },
  { target: "MasteryPct", transform: "MasteryToMasteryPct", raw: "Mastery" },
  { target: "BlockPct", transform: "BlockToBlockRate", raw: "Block" },
];

const attributeMappings = [];
let canonicalMappingInstructionSignature;
for (const file of audit.files || []) {
  for (const candidate of file.matches || []) {
    const constants = candidate.constants || [];
    if (!expectedAttributeMappings.every(({ target, transform, raw }) =>
      constants.includes(target) && constants.includes(transform) && constants.includes(raw))) {
      continue;
    }
    const instructions = candidate.instructions || [];
    for (let index = 0; index < expectedAttributeMappings.length; index += 1) {
      const mapping = expectedAttributeMappings[index];
      const transformPc = 20 + index * 4;
      requireInstruction(instructions, transformPc, "GETTABUP");
      requireInstruction(instructions, transformPc + 1, "GETTABLE");
      requireInstruction(instructions, transformPc + 2, "GETTABLE", undefined,
        { constantReference: mapping.target });
      requireInstruction(instructions, transformPc + 3, "SETTABLE", undefined,
        { constantReference: mapping.transform });

      const rawPc = 45 + index * 7;
      requireInstruction(instructions, rawPc, "GETTABUP");
      requireInstruction(instructions, rawPc + 1, "GETTABLE");
      requireInstruction(instructions, rawPc + 2, "GETTABLE", undefined,
        { constantReference: mapping.target });
      requireInstruction(instructions, rawPc + 3, "GETTABUP");
      requireInstruction(instructions, rawPc + 4, "GETTABLE");
      requireInstruction(instructions, rawPc + 5, "GETTABLE", undefined,
        { constantReference: mapping.raw });
      requireInstruction(instructions, rawPc + 6, "SETTABLE");
    }
    const signature = instructions
      .filter((row) => Number(row.pc) >= 20 && Number(row.pc) <= 86)
      .map((row) => row.raw)
      .join(":");
    if (canonicalMappingInstructionSignature === undefined) {
      canonicalMappingInstructionSignature = signature;
    } else {
      assert(signature === canonicalMappingInstructionSignature,
        "the two FightAttr mapping initializers no longer have identical bytecode");
    }
    attributeMappings.push({
      file: relative(file.file),
      function_path: (candidate.function_path || []).join("."),
      instruction_count: instructions.length,
      transform_mapping_pc_range: [20, 43],
      raw_attribute_mapping_pc_range: [45, 86],
    });
  }
}

const consumers = [];
let canonicalInstructionSignature;
for (const file of audit.files || []) {
  for (const candidate of file.matches || []) {
    const constants = candidate.constants || [];
    if (!constants.includes("FightAttrTranTableMgr") || !constants.includes("GetCurrentSeasonId")) {
      continue;
    }
    assert(constants[19] === 1 && constants[20] === 2 && constants[35] === 3,
      "FightAttrTran parameter indices 1-3 changed");
    assert(constants[38] === 4 && constants[39] === 5,
      "FightAttrTran season-cap parameter indices changed");
    assert(constants[40] === 6 && constants[41] === 7,
      "FightAttrTran role-level-cap parameter indices changed");
    assert(constants[42] === 100, "FightAttrTran percentage scale changed");
    assert(constants[54] === 0.01, "FightAttrTran display truncation quantum changed");

    const instructions = candidate.instructions || [];
    requireInstruction(instructions, 68, "GETTABUP");
    requireInstruction(instructions, 71, "LOADK", undefined, { constant: "season" });
    requireInstruction(instructions, 72, "CALL");
    requireInstruction(instructions, 75, "GETTABUP");
    requireInstruction(instructions, 78, "LOADK", undefined, { constant: "FightAttrTranTableMgr" });
    requireInstruction(instructions, 81, "MOVE");
    requireInstruction(instructions, 82, "CALL");

    const exactEvaluator = [
      [95, "GETTABLE"], [96, "MUL"],
      [97, "GETTABLE"], [98, "MUL"],
      [99, "GETTABLE"], [100, "ADD"],
      [101, "GETTABUP"], [102, "GETTABLE"],
      [103, "GETTABLE"], [104, "MUL"],
      [105, "GETTABLE"], [106, "CALL"],
      [107, "ADD"], [108, "GETTABUP"],
      [109, "GETTABLE"], [110, "GETTABLE"],
      [111, "MUL"], [112, "GETTABLE"],
      [113, "CALL"], [114, "ADD"],
      [115, "DIV"], [116, "MUL"],
    ];
    for (const [pc, opcode] of exactEvaluator) {
      requireInstruction(instructions, pc, opcode);
    }
    requireInstruction(instructions, 137, "MOD");
    requireInstruction(instructions, 138, "SUB");
    const instructionSignature = instructions
      .filter((row) => Number(row.pc) >= 50 && Number(row.pc) <= 142)
      .map((row) => row.raw)
      .join(":");
    if (canonicalInstructionSignature === undefined) {
      canonicalInstructionSignature = instructionSignature;
    } else {
      assert(instructionSignature === canonicalInstructionSignature,
        "the two FightAttrTran UI consumers no longer have identical evaluator bytecode");
    }

    consumers.push({
      file: relative(file.file),
      function_path: (candidate.function_path || []).join("."),
      line_defined: Number(candidate.line_defined),
      last_line_defined: Number(candidate.last_line_defined),
      instruction_count: instructions.length,
      evaluator_pc_range: [95, 116],
      display_truncation_pc_range: [137, 138],
    });
  }
}

assert(Number(audit.summary?.parse_failures || 0) === 0, "Lua audit contains parse failures");
assert(consumers.length === 2, `expected two identical UI consumers, found ${consumers.length}`);
assert(consumers.every((row) => row.instruction_count === 159),
  "FightAttrTran consumer instruction count changed");
assert(attributeMappings.length === 2,
  `expected two identical attribute mapping initializers, found ${attributeMappings.length}`);

const requiredFields = [
  "DefPara",
  "RefDefPara",
  "ElementDefToDamRes",
  "PhyPowerToDam",
  "MagPowerToDam",
  "CriToCrit",
  "HasteToHastePct",
  "LuckToLuckyStrikeProb",
  "VersatilityToVersatilityPct",
  "MasteryToMasteryPct",
  "BlockToBlockRate",
];
const rows = Object.values(table)
  .sort((left, right) => Number(left.Id) - Number(right.Id))
  .map((row) => {
    const fields = {};
    for (const field of requiredFields) {
      const parameters = (row[field] || []).map(Number);
      if (parameters.length === 0) {
        fields[field] = { state: "empty-current-build-parameter-array", parameters };
        continue;
      }
      assert(parameters.length === 7, `season ${row.Id} ${field} no longer has seven parameters`);
      fields[field] = {
        state: "exact-current-build-parameter-array",
        parameters,
        exact_expression:
          "100 * raw * p3 / (raw * p2 + p1 + min(season_level * p4, p5) + min(role_level * p6, p7))",
      };
    }
    return { season_id: Number(row.Id), fields };
  });

assert(rows.length === 3, `expected three FightAttrTran season rows, found ${rows.length}`);
assert(rows.map((row) => row.season_id).join(",") === "1,2,3",
  "FightAttrTran season row identities changed");

const numericAttributeMappings = expectedAttributeMappings.map((mapping) => {
  const target = findFightAttribute(fightAttrTable, mapping.target);
  const raw = findFightAttribute(fightAttrTable, mapping.raw);
  return {
    target_enum: mapping.target,
    target_attribute_id: Number(target.Id),
    transform_field: mapping.transform,
    raw_enum: mapping.raw,
    raw_attribute_id: Number(raw.Id),
  };
});
const hasteMapping = numericAttributeMappings.find((row) => row.transform_field === "HasteToHastePct");
assert(hasteMapping?.target_attribute_id === 11930,
  `HastePct numeric identity changed from 11930 to ${hasteMapping?.target_attribute_id}`);
assert(hasteMapping?.raw_attribute_id === 11120,
  `Haste numeric identity changed from 11120 to ${hasteMapping?.raw_attribute_id}`);

const result = {
  schema_version: 2,
  generated_by: "tools/fight-attribute-transform-evaluator-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: String(options.gameBuild),
  proof_state: "exact-current-build-client-ui-evaluator",
  policy: {
    executes_game_code: false,
    unresolved_evidence_is_hidden: false,
    row_selection_authority: "current season ID from the client season model",
    attribute_to_transform_mapping_is_exact: true,
    raw_to_transformed_attribute_mapping_is_exact: true,
    formula_operation_order_is_exact: true,
    table_parameter_values_are_exact: true,
    ui_display_truncation_is_not_runtime_counterfactual_rounding: true,
    combat_damage_stage_authority: false,
    promotion_requirement:
      "combat-side evidence must separately prove where each transformed value enters damage and its integer rounding",
  },
  inputs: {
    lua_audit: relative(auditPath),
    fight_attr_tran_table: relative(tablePath),
    fight_attr_table: relative(fightAttrTablePath),
  },
  summary: {
    exact_consumers: consumers.length,
    exact_attribute_mapping_initializers: attributeMappings.length,
    exact_numeric_attribute_mappings: numericAttributeMappings.length,
    season_rows: rows.length,
    proven_transform_fields: requiredFields.length,
    evaluator_formula:
      "100 * raw * p3 / (raw * p2 + p1 + min(season_level * p4, p5) + min(role_level * p6, p7))",
    row_selection: "FightAttrTranTable[current_season_id]",
    underlying_value_rounding: "no rounding in the proven evaluator",
    display_only_rounding: "value - (value % 0.01)",
  },
  consumers,
  attribute_mapping_initializers: attributeMappings,
  numeric_attribute_mappings: numericAttributeMappings,
  rows,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function requireInstruction(instructions, pc, opcode, line, extra = {}) {
  const instruction = instructions.find((row) => Number(row.pc) === pc);
  assert(instruction, `missing instruction pc ${pc}`);
  assert(instruction.opcode === opcode, `pc ${pc} changed from ${opcode} to ${instruction.opcode}`);
  if (line !== undefined) {
    assert(Number(instruction.line) === line, `pc ${pc} source line changed from ${line}`);
  }
  if (Object.hasOwn(extra, "constant")) {
    assert(instruction.operands?.constant === extra.constant,
      `pc ${pc} constant changed from ${extra.constant}`);
  }
  if (Object.hasOwn(extra, "constantReference")) {
    const references = Object.values(instruction.operands || {})
      .filter((value) => typeof value === "string");
    assert(references.some((value) => value.includes(`='${extra.constantReference}'`)),
      `pc ${pc} no longer references constant ${extra.constantReference}`);
  }
  return instruction;
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    if (!key?.startsWith("--") || args[index + 1] === undefined) {
      throw new Error(`invalid argument near ${key ?? "<end>"}`);
    }
    parsed[key.slice(2)] = args[index + 1];
  }
  for (const required of ["gameBuild", "audit", "table", "fightAttrTable", "output"]) {
    if (!parsed[required]) throw new Error(`--${required} is required`);
  }
  return parsed;
}

function findFightAttribute(table, enumName) {
  const exactName = `Attr${enumName}`;
  const rows = Object.values(table || {}).filter((row) => row?.EnumName === exactName);
  assert(rows.length === 1, `expected exactly one FightAttrTable row for ${exactName}, found ${rows.length}`);
  return rows[0];
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relative(value) {
  const normalized = path.relative(repoRoot, value).replaceAll("\\", "/");
  return normalized.startsWith("../") ? value.replaceAll("\\", "/") : normalized;
}

function readJson(value, label) {
  try {
    return JSON.parse(readFileSync(value, "utf8"));
  } catch (error) {
    throw new Error(`failed to read ${label} at ${value}: ${error.message}`);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
