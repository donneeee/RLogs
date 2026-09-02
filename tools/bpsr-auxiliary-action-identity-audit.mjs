#!/usr/bin/env node

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const GAME_DATA = path.join(
  ROOT,
  "plugins/games/blue-protocol-star-resonance/game-data",
);
const PROOF_PATH = path.join(
  GAME_DATA,
  "runtime/auxiliary-action-identity-proof.v1.json",
);
const PRESENTATION_PATH = path.join(
  GAME_DATA,
  "runtime/auxiliary-action-presentation.v1.json",
);
const SOURCE_PATH = path.join(
  GAME_DATA,
  "catalog/loadouts/auxiliary-actions.v1.json",
);
const IMAGINE_PRESENTATION_PATH = path.join(
  GAME_DATA,
  "runtime/battle-imagine-presentation.v1.json",
);
const AUXILIARY_NAMES_PATH = path.join(
  GAME_DATA,
  "runtime/localization/en-US/auxiliary-action-names.v1.json",
);
const IMAGINE_NAMES_PATH = path.join(
  GAME_DATA,
  "runtime/localization/en-US/battle-imagine-names.v1.json",
);

const command = process.argv[2] ?? "verify";
if (command !== "verify") {
  throw new Error("usage: node tools/bpsr-auxiliary-action-identity-audit.mjs verify");
}

const proof = readJson(PROOF_PATH);
const presentation = readJson(PRESENTATION_PATH);
const source = readJson(SOURCE_PATH);
const imaginePresentation = readJson(IMAGINE_PRESENTATION_PATH);
const auxiliaryNames = readJson(AUXILIARY_NAMES_PATH);
const imagineNames = readJson(IMAGINE_NAMES_PATH);

verifyProofShape(proof);
verifyCatalogs(proof, presentation, source, imaginePresentation, auxiliaryNames, imagineNames);

console.log(
  `verified build ${proof.game_build}: ${proof.pairs.length} distinct role-action/Imagine-action pairs, `
    + `role tiers T1-T${proof.policy.role_imagine_maximum_tier}, `
    + `Battle Imagine tiers T0-T${proof.policy.battle_imagine_maximum_tier}`,
);

function verifyProofShape(value) {
  assert(value.schema_version === 1, "unexpected proof schema");
  assert(value.game === "blue-protocol-star-resonance", "unexpected proof game");
  assert(value.deployment_id === "global", "unexpected proof deployment");
  assert(value.channel === "steam", "unexpected proof channel");
  assert(/^\d+$/.test(value.game_build), "proof game build is invalid");
  assert(value.policy?.role_action_and_normal_imagine_action_ids_are_distinct === true,
    "proof must require distinct role and normal Imagine action IDs");
  assert(value.policy?.role_imagine_maximum_tier === 4,
    "role Imagine tier domain must end at T4");
  assert(value.policy?.battle_imagine_maximum_tier === 5,
    "normal Battle Imagine tier domain must end at T5");
  assert(value.policy?.normal_imagine_name_prefix === "Arcane! ",
    "normal Imagine action name normalization changed");
  assert(Array.isArray(value.pairs) && value.pairs.length === 8,
    "the exact role Imagine identity set must contain eight pairs");

  for (const sourceRecord of Object.values(value.sources ?? {})) {
    assert(isSha256(sourceRecord.sha256), "a proof source is missing its exact SHA-256");
    if ("semantic_sha256" in sourceRecord) {
      assert(isSha256(sourceRecord.semantic_sha256),
        "a proof source has an invalid semantic SHA-256");
    }
  }

  const roleIds = [];
  const normalIds = [];
  const itemIds = [];
  for (const pair of value.pairs) {
    assert(Number.isInteger(pair.role_action_id) && pair.role_action_id > 0,
      "a role action ID is invalid");
    assert(Number.isInteger(pair.normal_imagine_action_id) && pair.normal_imagine_action_id > 0,
      "a normal Imagine action ID is invalid");
    assert(pair.role_action_id !== pair.normal_imagine_action_id,
      `role action ${pair.role_action_id} reuses its normal Imagine action ID`);
    assert(Number.isInteger(pair.battle_imagine_item_id) && pair.battle_imagine_item_id > 0,
      "a Battle Imagine item ID is invalid");
    assert(nonEmpty(pair.role_action_name) && nonEmpty(pair.normal_imagine_action_name)
      && nonEmpty(pair.battle_imagine_name), "a proof pair is missing a name");
    assert(isSha256(pair.role_action_row_sha256),
      `role action ${pair.role_action_id} has no exact row fingerprint`);
    assert(isSha256(pair.normal_imagine_action_row_sha256),
      `normal Imagine action ${pair.normal_imagine_action_id} has no exact row fingerprint`);
    assert(pair.normal_imagine_action_name.startsWith(value.policy.normal_imagine_name_prefix),
      `normal Imagine action ${pair.normal_imagine_action_id} is missing its exact prefix`);
    assert(
      pair.normal_imagine_action_name
        .slice(value.policy.normal_imagine_name_prefix.length) === pair.role_action_name,
      `role action ${pair.role_action_id} does not normalize to normal Imagine action ${pair.normal_imagine_action_id}`,
    );
    roleIds.push(pair.role_action_id);
    normalIds.push(pair.normal_imagine_action_id);
    itemIds.push(pair.battle_imagine_item_id);
  }
  assert(strictlyIncreasing(roleIds), "role action IDs must be sorted and unique");
  assert(unique(normalIds), "normal Imagine action IDs must be unique");
  assert(unique(itemIds), "Battle Imagine item IDs must be unique");
  assert(roleIds.every((id) => !normalIds.includes(id)),
    "role and normal Imagine action ID domains overlap");
}

function verifyCatalogs(
  proofValue,
  presentationValue,
  sourceValue,
  imaginePresentationValue,
  auxiliaryNamesValue,
  imagineNamesValue,
) {
  assert(presentationValue.schema_version === 1, "unexpected auxiliary presentation schema");
  assert(sourceValue.schema_version === 1, "unexpected auxiliary source schema");
  assert(imaginePresentationValue.schema_version === 1,
    "unexpected Battle Imagine presentation schema");
  assert(auxiliaryNamesValue.schema_version === 1 && auxiliaryNamesValue.locale === "en-US",
    "unexpected auxiliary localization catalog");
  assert(imagineNamesValue.schema_version === 1 && imagineNamesValue.locale === "en-US",
    "unexpected Battle Imagine localization catalog");
  assert(Array.isArray(presentationValue.skills) && presentationValue.skills.length === 20,
    "runtime auxiliary presentation must contain all 20 role actions");
  assert(Array.isArray(sourceValue.actions) && sourceValue.actions.length === 20,
    "reviewed auxiliary source must contain all 20 role actions");

  const runtimeByRoleId = indexBy(presentationValue.skills, "skill_id");
  const sourceByRoleId = indexBy(sourceValue.actions, "skill_id");
  const imagineByActionId = indexBy(imaginePresentationValue.imagines, "skill_id");
  const auxiliaryNameById = new Map(auxiliaryNamesValue.skills);
  const imagineNameByItemId = new Map(imagineNamesValue.imagines);
  const proofByRoleId = indexBy(proofValue.pairs, "role_action_id");

  for (const runtime of presentationValue.skills) {
    const reviewed = sourceByRoleId.get(runtime.skill_id);
    assert(reviewed != null, `runtime role action ${runtime.skill_id} is not reviewed`);
    assert(runtime.icon === reviewed.icon,
      `runtime role action ${runtime.skill_id} icon drifted from its reviewed source`);
    assert(runtime.action_kind === reviewed.kind,
      `runtime role action ${runtime.skill_id} kind drifted from its reviewed source`);
    assert(runtime.replacement_imagine_skill_id === reviewed.replacement_imagine_skill_id,
      `runtime role action ${runtime.skill_id} linked Imagine action drifted`);

    const pair = proofByRoleId.get(runtime.skill_id);
    if (runtime.action_kind === "role_skill") {
      assert(pair == null && runtime.maximum_tier === null
        && runtime.replacement_imagine_skill_id === null,
      `native role action ${runtime.skill_id} incorrectly claims Imagine identity`);
    } else {
      assert(runtime.action_kind === "role_imagine",
        `role action ${runtime.skill_id} has an unsupported kind`);
      assert(pair != null, `Imagine role action ${runtime.skill_id} lacks build proof`);
      assert(runtime.maximum_tier === proofValue.policy.role_imagine_maximum_tier,
        `Imagine role action ${runtime.skill_id} has the wrong maximum tier`);
      assert(runtime.replacement_imagine_skill_id === pair.normal_imagine_action_id,
        `Imagine role action ${runtime.skill_id} points to the wrong normal action UID`);
    }
  }

  for (const pair of proofValue.pairs) {
    assert(auxiliaryNameById.get(pair.role_action_id) === pair.role_action_name,
      `role action ${pair.role_action_id} localization drifted`);
    const imagine = imagineByActionId.get(pair.normal_imagine_action_id);
    assert(imagine != null,
      `normal Imagine action ${pair.normal_imagine_action_id} has no Battle Imagine presentation`);
    assert(imagine.item_id === pair.battle_imagine_item_id,
      `normal Imagine action ${pair.normal_imagine_action_id} points to the wrong item`);
    assert(imagine.maximum_tier === proofValue.policy.battle_imagine_maximum_tier,
      `Battle Imagine ${pair.battle_imagine_item_id} has the wrong maximum tier`);
    assert(
      stripBattleImaginePrefix(imagineNameByItemId.get(pair.battle_imagine_item_id))
        === pair.battle_imagine_name,
      `Battle Imagine item ${pair.battle_imagine_item_id} localization drifted`,
    );
  }
}

function readJson(file) {
  return JSON.parse(readFileSync(file, "utf8"));
}

function indexBy(values, key) {
  const result = new Map();
  for (const value of values) {
    assert(!result.has(value[key]), `duplicate ${key} ${value[key]}`);
    result.set(value[key], value);
  }
  return result;
}

function stripBattleImaginePrefix(value) {
  return String(value ?? "").replace(/^Battle Imagine\s*-\s*/i, "").trim();
}

function strictlyIncreasing(values) {
  return values.every((value, index) => index === 0 || values[index - 1] < value);
}

function unique(values) {
  return new Set(values).size === values.length;
}

function nonEmpty(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
