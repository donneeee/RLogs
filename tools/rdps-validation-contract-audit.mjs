#!/usr/bin/env node

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const manifestPath = resolvePath(options.manifest);
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const obligations = manifest.obligations || [];
const errors = [];
const ids = new Set();
const runtimeCohorts = new Map();
const routeAudited = new Set();

for (const obligation of obligations) {
  const id = String(obligation.obligation_id || "");
  if (!id) errors.push("an obligation has no obligation_id");
  if (ids.has(id)) errors.push(`duplicate obligation_id ${id}`);
  ids.add(id);

  const selectors = obligation.selectors || {};
  const sourceRules = uniqueStrings(selectors.source_rule_ids);
  const effectIds = uniqueNumbers(selectors.effect_ids);
  const sourceConfigIds = uniqueNumbers(selectors.source_config_ids);
  const suitPairs = uniqueSuitPairs(selectors.equipment_suit_entries);
  const required = new Set(obligation.required_event_kinds || []);
  const domain = String(obligation.domain || "");
  const validationRoute = String(obligation.evidence?.validation_route || "");

  if (required.has("status") && effectIds.length === 0) {
    errors.push(`${id} requires status without an exact effect selector`);
  }
  if (required.has("profile_selection") && uniqueNumbers(selectors.item_ids).length === 0) {
    errors.push(`${id} requires profile_selection without an exact item selector`);
  }
  if (required.has("formula_inputs") && (obligation.formula_inputs || []).length === 0) {
    errors.push(`${id} requires formula_inputs without formula input definitions`);
  }
  if ((obligation.formula_inputs || []).length > 0 && !required.has("formula_inputs")) {
    errors.push(`${id} defines formula inputs without requiring formula_inputs`);
  }
  for (const pair of suitPairs) {
    if (pair.map_key <= 0 || pair.attribute_key <= 0) {
      errors.push(`${id} has a non-positive equipment suit selector`);
    }
    if (!sourceConfigIds.includes(pair.attribute_key)) {
      errors.push(`${id} equipment suit ${pair.map_key}:${pair.attribute_key} lacks its source_config_id`);
    }
  }
  if (sourceConfigIds.length > 0 && suitPairs.length === 0) {
    errors.push(`${id} has source_config_ids without an exact equipment suit pair`);
  }

  const routeErrorsBefore = errors.length;
  if (domain === "psychoscope-factor") {
    if (!required.has("profile_selection") || uniqueNumbers(selectors.item_ids).length === 0) {
      errors.push(`${id} psychoscope factor lacks exact profile selection`);
    }
    if (required.has("damage") && effectIds.length === 0
      && uniqueNumbers(selectors.skill_ids).length === 0
      && uniqueNumbers(selectors.damage_ids).length === 0
      && uniqueNumbers(selectors.recount_ids).length === 0) {
      errors.push(`${id} requires damage without a status or packet-output selector`);
    }
  } else if (domain === "packet-output-route") {
    if (!required.has("damage") || !required.has("status") || !required.has("formula_inputs")) {
      errors.push(`${id} packet output must require damage, status, and formula inputs`);
    }
    for (const input of obligation.formula_inputs || []) {
      if (input.actor_role !== "source"
        || input.completion !== "any-current-value-observed-before-trigger"
        || uniqueNumbers(input.candidate_attribute_ids).length === 0) {
        errors.push(`${id} has an unsupported formula input contract for ${input.input_key}`);
      }
    }
  } else if (domain === "target-mitigation") {
    if (!["damage", "entity_attributes", "temporary_attributes"].every((kind) => required.has(kind))) {
      errors.push(`${id} target mitigation lacks damage and both attribute lanes`);
    }
  } else if (domain === "mastery-property") {
    validateMasteryRoute(id, validationRoute, required, selectors, obligation.evidence || {}, errors);
  } else if (domain === "offensive-runtime-gate") {
    if (!required.has("damage") || !required.has("status")) {
      errors.push(`${id} offensive runtime gate lacks damage/status evidence`);
    }
  } else {
    errors.push(`${id} uses an unaudited domain ${domain || "<empty>"}`);
  }
  if (errors.length === routeErrorsBefore) routeAudited.add(id);

  const signature = stableJson(runtimeSelectors(selectors));
  const cohort = runtimeCohorts.get(signature) || { obligationIds: [], sourceRules: new Set() };
  cohort.obligationIds.push(id);
  for (const sourceRule of sourceRules) cohort.sourceRules.add(sourceRule);
  runtimeCohorts.set(signature, cohort);
}

const crossSourceCollisions = [...runtimeCohorts.values()]
  .filter((cohort) => cohort.sourceRules.size > 1);
for (const cohort of crossSourceCollisions) {
  errors.push(
    `runtime selector collision spans source rules ${[...cohort.sourceRules].join(", ")}: ${cohort.obligationIds.join(", ")}`,
  );
}

const summary = {
  manifest: path.relative(repoRoot, manifestPath).replaceAll("\\", "/"),
  obligations: obligations.length,
  runtime_selector_cohorts: runtimeCohorts.size,
  multi_obligation_runtime_selector_cohorts: [...runtimeCohorts.values()]
    .filter((cohort) => cohort.obligationIds.length > 1).length,
  cross_source_runtime_selector_collisions: crossSourceCollisions.length,
  route_audited_obligations: routeAudited.size,
  route_unaudited_obligations: obligations.length - routeAudited.size,
  contract_errors: errors.length,
};
console.log(JSON.stringify(summary));
if (errors.length > 0) {
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

function validateMasteryRoute(id, route, required, selectors, evidence, errors) {
  const exactOutput = uniqueNumbers([
    ...(selectors.skill_ids || []),
    ...(selectors.damage_ids || []),
    ...(selectors.recount_ids || []),
  ]).length > 0;
  const exactStatus = uniqueNumbers(selectors.effect_ids).length > 0;
  const common = ["actor", "entity_attributes"];
  if (!common.every((kind) => required.has(kind))) {
    errors.push(`${id} Mastery route lacks actor/entity attribute identity`);
  }
  const supported = new Set([
    "outgoing-damage", "outgoing-selected-ability-damage",
    "owned-companion-outgoing-damage", "outgoing-healing",
    "outgoing-shield-or-barrier-state", "named-shield-state",
    "incoming-damage-mitigation", "owned-resource-transition",
    "selected-ability-cooldown-transition", "named-skill-output",
    "named-status-lifecycle", "named-resource-decay-lifecycle",
  ]);
  if (!supported.has(route)) {
    errors.push(`${id} uses unsupported Mastery validation route ${route || "<empty>"}`);
    return;
  }
  const needs = (...kinds) => {
    if (!kinds.every((kind) => required.has(kind))) {
      errors.push(`${id} ${route} lacks ${kinds.join("+")} evidence`);
    }
  };
  if (["outgoing-damage", "owned-companion-outgoing-damage", "incoming-damage-mitigation"].includes(route)) {
    needs("damage");
  } else if (["outgoing-selected-ability-damage", "named-skill-output"].includes(route)) {
    needs("damage");
    if (!exactOutput) errors.push(`${id} ${route} lacks an exact output selector`);
  } else if (route === "outgoing-healing") {
    needs("healing");
  } else if (route === "outgoing-shield-or-barrier-state") {
    needs("shield_state");
  } else if (route === "owned-resource-transition") {
    needs("resource");
  } else if (route === "selected-ability-cooldown-transition") {
    needs("cooldown");
    if (!exactOutput) errors.push(`${id} cooldown route lacks an exact skill selector`);
  } else if (route === "named-status-lifecycle") {
    needs("status");
    if (!exactStatus) errors.push(`${id} named status route lacks an exact effect selector`);
  } else if (route === "named-shield-state") {
    needs("status", "shield_state");
    if (!exactStatus) errors.push(`${id} named shield route lacks an exact effect selector`);
  } else if (route === "named-resource-decay-lifecycle") {
    needs("status", "resource");
    if (!exactStatus) errors.push(`${id} named resource route lacks an exact effect selector`);
  }
  if (route === "incoming-damage-mitigation"
    && evidence.component_kind === "all-element-resistance"
    && uniqueNumbers(evidence.property_ids).length === 0) {
    errors.push(`${id} elemental mitigation lacks exact property selectors`);
  }
}

function runtimeSelectors(selectors) {
  return {
    effect_ids: uniqueNumbers(selectors.effect_ids),
    skill_ids: uniqueNumbers(selectors.skill_ids),
    damage_ids: uniqueNumbers(selectors.damage_ids),
    recount_ids: uniqueNumbers(selectors.recount_ids),
    attribute_ids: uniqueNumbers(selectors.attribute_ids),
    class_ids: uniqueNumbers(selectors.class_ids),
    specialization_ids: uniqueNumbers(selectors.specialization_ids),
    item_ids: uniqueNumbers(selectors.item_ids),
    source_config_ids: uniqueNumbers(selectors.source_config_ids),
    equipment_suit_entries: uniqueSuitPairs(selectors.equipment_suit_entries),
  };
}

function uniqueNumbers(values) {
  return [...new Set((values || []).map(Number))].sort((left, right) => left - right);
}

function uniqueStrings(values) {
  return [...new Set((values || []).map(String))].sort();
}

function uniqueSuitPairs(values) {
  return [...new Map((values || []).map((value) => [
    `${Number(value.map_key)}:${Number(value.attribute_key)}`,
    { map_key: Number(value.map_key), attribute_key: Number(value.attribute_key) },
  ])).values()].sort((left, right) =>
    left.map_key - right.map_key || left.attribute_key - right.attribute_key
  );
}

function stableJson(value) {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function resolvePath(value) {
  if (!value) throw new Error("--manifest is required");
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) continue;
    const key = token.slice(2);
    const next = args[index + 1];
    parsed[key] = next && !next.startsWith("--") ? args[++index] : true;
  }
  return parsed;
}
