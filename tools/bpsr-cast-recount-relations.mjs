#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";

const SCHEMA_VERSION = 1;
const DAMAGE_MERGE_PATTERN = /damageMerge\(\{([^}]*)\}/g;

function usage() {
  return [
    "usage: node tools/bpsr-cast-recount-relations.mjs",
    "  --skill-table <SkillTable.json>",
    "  --skill-effect-table <SkillEffectTable.json>",
    "  --skill-fight-level-table <SkillFightLevelTable.json>",
    "  --recount-table <RecountTable.json>",
    "  --game-build <numeric-build>",
    "  --output <combat-cast-recount-relations.v1.json>",
  ].join(" ");
}

function parseArguments(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      throw new Error(usage());
    }
    values.set(key.slice(2), value);
  }
  const required = [
    "skill-table",
    "skill-effect-table",
    "skill-fight-level-table",
    "recount-table",
    "game-build",
    "output",
  ];
  for (const key of required) {
    if (!values.get(key)) {
      throw new Error(`missing --${key}\n${usage()}`);
    }
  }
  if (!/^\d+$/.test(values.get("game-build"))) {
    throw new Error("--game-build must be numeric");
  }
  return Object.fromEntries(values);
}

function readObject(filePath, label) {
  const value = JSON.parse(fs.readFileSync(filePath, "utf8"));
  if (!value || Array.isArray(value) || typeof value !== "object") {
    throw new Error(`${label} must be a JSON object`);
  }
  return value;
}

function normalizedName(value) {
  return String(value ?? "")
    .normalize("NFKC")
    .toLocaleLowerCase("en-US")
    .replace(/[^\p{L}\p{N}]+/gu, "");
}

function collectDamageMergeIds(value, damageToRecount, output) {
  if (typeof value === "string") {
    for (const match of value.matchAll(DAMAGE_MERGE_PATTERN)) {
      for (const rawId of match[1].split(",")) {
        const damageId = Number(rawId.trim());
        if (Number.isSafeInteger(damageId) && damageToRecount.has(damageId)) {
          output.add(damageId);
        }
      }
    }
    return;
  }
  if (Array.isArray(value)) {
    for (const child of value) {
      collectDamageMergeIds(child, damageToRecount, output);
    }
    return;
  }
  if (value && typeof value === "object") {
    for (const child of Object.values(value)) {
      collectDamageMergeIds(child, damageToRecount, output);
    }
  }
}

function insertRelation(relations, relation) {
  const previous = relations.get(relation.ability_id);
  if (previous && previous.recount_group_id !== relation.recount_group_id) {
    throw new Error(
      `ability ${relation.ability_id} resolves to both Recount groups ` +
        `${previous.recount_group_id} and ${relation.recount_group_id}`,
    );
  }
  if (!previous || previous.evidence_kind === "skill-effect-single-group") {
    relations.set(relation.ability_id, relation);
  }
}

function buildRelations(skillTable, skillEffectTable, skillFightLevelTable, recountTable) {
  const damageToRecount = new Map();
  const recountNames = new Map();
  for (const row of Object.values(recountTable)) {
    const recountGroupId = Number(row.Id);
    if (!Number.isSafeInteger(recountGroupId) || recountGroupId <= 0) {
      throw new Error("RecountTable contains an invalid Id");
    }
    recountNames.set(recountGroupId, String(row.RecountName ?? ""));
    for (const rawDamageId of row.DamageId ?? []) {
      const damageId = Number(rawDamageId);
      if (!Number.isSafeInteger(damageId) || damageId <= 0) {
        throw new Error(`Recount group ${recountGroupId} has an invalid DamageId`);
      }
      const previous = damageToRecount.get(damageId);
      if (previous !== undefined && previous !== recountGroupId) {
        throw new Error(
          `DamageAttr ${damageId} belongs to Recount groups ${previous} and ${recountGroupId}`,
        );
      }
      damageToRecount.set(damageId, recountGroupId);
    }
  }

  const fightLevelEffects = new Map();
  for (const row of Object.values(skillFightLevelTable)) {
    const skillId = Number(row.SkillId);
    const effectId = Number(row.SkillEffectId);
    if (!Number.isSafeInteger(skillId) || !Number.isSafeInteger(effectId)) {
      continue;
    }
    if (!fightLevelEffects.has(skillId)) {
      fightLevelEffects.set(skillId, new Set());
    }
    fightLevelEffects.get(skillId).add(effectId);
  }

  const relations = new Map();
  for (const [damageId, recountGroupId] of damageToRecount) {
    insertRelation(relations, {
      ability_id: damageId,
      recount_group_id: recountGroupId,
      evidence_kind: "recount-damage-id",
    });
  }

  let ambiguousSkillCount = 0;
  const skillRows = new Map();
  const mappedSkillGroups = new Map();
  for (const row of Object.values(skillTable)) {
    const skillId = Number(row.Id);
    if (!Number.isSafeInteger(skillId) || skillId <= 0) {
      continue;
    }
    skillRows.set(skillId, row);
    const effectIds = new Set([
      ...(row.EffectIDs ?? []).map(Number),
      ...(fightLevelEffects.get(skillId) ?? []),
    ]);
    const referencedDamageIds = new Set();
    for (const effectId of effectIds) {
      const effect = skillEffectTable[effectId];
      if (effect) {
        collectDamageMergeIds(effect, damageToRecount, referencedDamageIds);
      }
    }
    const recountGroupIds = [
      ...new Set([...referencedDamageIds].map((damageId) => damageToRecount.get(damageId))),
    ];
    let recountGroupId;
    let evidenceKind;
    if (recountGroupIds.length === 1) {
      [recountGroupId] = recountGroupIds;
      evidenceKind = "skill-effect-single-group";
    } else if (recountGroupIds.length > 1) {
      const skillName = normalizedName(row.Name);
      const matchingGroups = recountGroupIds.filter(
        (candidate) => normalizedName(recountNames.get(candidate)) === skillName,
      );
      if (skillName && matchingGroups.length === 1) {
        [recountGroupId] = matchingGroups;
        evidenceKind = "skill-effect-name-match";
      } else {
        ambiguousSkillCount += 1;
      }
    }
    if (recountGroupId !== undefined) {
      mappedSkillGroups.set(skillId, recountGroupId);
      insertRelation(relations, {
        ability_id: skillId,
        recount_group_id: recountGroupId,
        evidence_kind: evidenceKind,
      });
    }
  }

  // Combo stages are separate UseSkill request IDs, but SkillTable links them
  // through NextSkillId. Propagate only into an otherwise-unmapped stage and
  // stop on any independently resolved child, so transformed skills that own
  // a different Recount group retain that exact identity.
  let ambiguousNextSkillCount = 0;
  for (let pass = 0; pass < skillRows.size; pass += 1) {
    const candidates = new Map();
    for (const [skillId, row] of skillRows) {
      const recountGroupId = mappedSkillGroups.get(skillId);
      const nextSkillId = Number(row.NextSkillId);
      if (
        recountGroupId === undefined ||
        !Number.isSafeInteger(nextSkillId) ||
        nextSkillId <= 0 ||
        mappedSkillGroups.has(nextSkillId)
      ) {
        continue;
      }
      if (!candidates.has(nextSkillId)) {
        candidates.set(nextSkillId, new Set());
      }
      candidates.get(nextSkillId).add(recountGroupId);
    }
    let added = 0;
    for (const [nextSkillId, recountGroupIds] of candidates) {
      if (recountGroupIds.size !== 1) {
        ambiguousNextSkillCount += 1;
        continue;
      }
      const [recountGroupId] = recountGroupIds;
      const existing = relations.get(nextSkillId);
      if (existing && existing.recount_group_id !== recountGroupId) {
        ambiguousNextSkillCount += 1;
        continue;
      }
      mappedSkillGroups.set(nextSkillId, recountGroupId);
      insertRelation(relations, {
        ability_id: nextSkillId,
        recount_group_id: recountGroupId,
        evidence_kind: "skill-next-chain",
      });
      added += 1;
    }
    if (added === 0) {
      break;
    }
  }

  return {
    relations: [...relations.values()].sort((left, right) => left.ability_id - right.ability_id),
    damageIdCount: damageToRecount.size,
    ambiguousSkillCount: ambiguousSkillCount + ambiguousNextSkillCount,
  };
}

function writeJsonAtomic(filePath, value) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  const temporaryPath = `${filePath}.tmp-${process.pid}`;
  fs.writeFileSync(temporaryPath, `${JSON.stringify(value)}\n`, "utf8");
  fs.renameSync(temporaryPath, filePath);
}

try {
  const args = parseArguments(process.argv.slice(2));
  const sourcePaths = {
    SkillTable: args["skill-table"],
    SkillEffectTable: args["skill-effect-table"],
    SkillFightLevelTable: args["skill-fight-level-table"],
    RecountTable: args["recount-table"],
  };
  const result = buildRelations(
    readObject(args["skill-table"], "SkillTable"),
    readObject(args["skill-effect-table"], "SkillEffectTable"),
    readObject(args["skill-fight-level-table"], "SkillFightLevelTable"),
    readObject(args["recount-table"], "RecountTable"),
  );
  writeJsonAtomic(args.output, {
    schema_version: SCHEMA_VERSION,
    game_build: args["game-build"],
    generation_scope: "all-exact-current-build-recount-damage-and-unambiguous-cast-relations",
    source_sha256: Object.fromEntries(
      Object.entries(sourcePaths).map(([label, filePath]) => [
        label,
        createHash("sha256").update(fs.readFileSync(filePath)).digest("hex"),
      ]),
    ),
    relations: result.relations,
  });
  console.log(
    `wrote ${result.relations.length} relations (${result.damageIdCount} exact damage IDs; ` +
      `${result.ambiguousSkillCount} ambiguous skills withheld) to ${args.output}`,
  );
} catch (error) {
  console.error(`BPSR cast/Recount relation build failed: ${error.message}`);
  process.exitCode = 1;
}
