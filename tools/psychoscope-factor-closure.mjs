#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const options = parseArgs(process.argv.slice(2));
const factorsPath = requiredPath(options.factors, "--factors");
const recountPath = requiredPath(options.recount, "--recount");
const skillsPath = requiredPath(options.skills, "--skills");
const outputPath = requiredPath(options.output, "--output");
const gameBuild = String(options.build || "").trim();
if (!/^\d+$/.test(gameBuild)) throw new Error("--build must be a numeric client build");

const factors = readJson(factorsPath);
const recount = readJson(recountPath);
const skills = readJson(skillsPath);
const recountRows = Object.values(recount).filter(isObject);
const skillRows = Object.values(skills).filter(isObject);
const recountIndexes = buildLocaleIndexes(recountRows, (row) => row.Names, (row) => row.Name || row.RecountName);
const skillIndexes = buildLocaleIndexes(skillRows, (row) => row.Names, (row) => row.Name);

const families = Object.values(factors.factorFamiliesById || {})
  .filter(isObject)
  .sort((left, right) => Number(left.familyId) - Number(right.familyId))
  .map(traceFamily);

const byCategory = countBy(families, (row) => row.slot_category || "unclassified");
const byRuntimeRole = countBy(families, (row) => row.runtime_role || "unclassified");
const byRouteState = countBy(families, (row) => row.offline_route_state);
const byMechanic = {};
for (const family of families) {
  for (const mechanic of family.mechanic_classes) {
    byMechanic[mechanic] = (byMechanic[mechanic] || 0) + 1;
  }
}

const result = {
  schema_version: 1,
  generated_by: "tools/psychoscope-factor-closure.mjs",
  game: "blue-protocol-star-resonance",
  game_build: gameBuild,
  policy: {
    descriptions_identify_candidates_only: true,
    exact_ids_or_packet_events_are_runtime_authority: true,
    unmatched_evidence_hidden: false,
    guessed_recount_relationships_allowed: false,
    capture_gate_is_global_not_factor_specific: true,
  },
  inputs: {
    factors: normalizePath(factorsPath),
    recount: normalizePath(recountPath),
    skills: normalizePath(skillsPath),
  },
  summary: {
    factor_families: families.length,
    current_runtime_families: families.filter((row) => row.current_runtime_eligible).length,
    archived_expired_families: families.filter((row) => !row.current_runtime_eligible).length,
    by_category: byCategory,
    by_runtime_role: byRuntimeRole,
    by_offline_route_state: byRouteState,
    by_mechanic_class: sortObject(byMechanic),
    reality_families: families.filter((row) => row.slot_category === "reality").length,
    inspiration_families: families.filter((row) => row.slot_category === "inspiration").length,
    reality_with_exact_recount_or_damage_route: families.filter((row) =>
      row.slot_category === "reality"
      && (row.direct_damage_ids.length || row.exact_recount_ids.length || row.generated_damage_families.length)
    ).length,
    reality_without_exact_output_route: families.filter((row) =>
      row.slot_category === "reality" && row.offline_route_state !== "exact-static-output-route"
    ).length,
    reality_with_exact_static_or_state_route: families.filter((row) =>
      row.slot_category === "reality" && row.offline_route_state !== "offline-output-route-needed"
    ).length,
    reality_without_exact_offline_route: families.filter((row) =>
      row.slot_category === "reality" && row.offline_route_state === "offline-output-route-needed"
    ).length,
    total_offline_route_obligations: families.filter((row) =>
      row.offline_route_state === "offline-output-route-needed"
    ).length,
    total_final_validation_obligations: families.filter((row) =>
      row.current_runtime_eligible && row.final_validation_obligations.length > 0
    ).length,
  },
  families,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary));

function traceFamily(family) {
  const buffIds = numbers((family.gradeRows || []).map((row) => row.primaryBuffId));
  const factorRows = buffIds.map((id) => factors.factorsByBuffId?.[String(id)]).filter(isObject);
  const descriptions = familyDescriptions(family, factorRows);
  const gradeRows = family.gradeRows || [];
  const expiredDescriptions = gradeRows.filter((row) =>
    isExpiredDescription(row.energy?.description)
  ).length;
  const currentRuntimeEligible = gradeRows.length > 0 && expiredDescriptions === 0;
  if (expiredDescriptions > 0 && expiredDescriptions !== gradeRows.length) {
    throw new Error(`factor family ${family.familyId} mixes expired and current grade descriptions`);
  }
  const recountMatches = collectLocalizedMatches(descriptions, recountIndexes, (row) => ({
    recount_id: Number(row.Id),
    recount_name: row.Name || row.RecountName || "",
    damage_ids: numbers(row.DamageId),
  }));
  const skillMatches = collectLocalizedMatches(descriptions, skillIndexes, (row) => ({
    skill_id: Number(row.Id),
    skill_name: row.Name || "",
    recount_ids: numbers(row.RecountIds),
    kinds: strings(row.Kinds || [row.Kind]),
  }));
  const directDamageIds = numbers(factorRows.flatMap((row) => row.affectedDamageIds || []));
  const declaredRecountIds = numbers(factorRows.flatMap((row) => row.affectedRecountIds || []));
  const semanticRecountIds = numbers(recountMatches.flatMap((row) => row.value.recount_id));
  const skillRecountIds = numbers(skillMatches.flatMap((row) => row.value.recount_ids));
  const exactRecountIds = numbers([...declaredRecountIds, ...semanticRecountIds, ...skillRecountIds]);
  const exactSkillIds = numbers(skillMatches.flatMap((row) => row.value.skill_id));
  const runtimeSelectors = uniqueObjects(factorRows.flatMap((row) => row.affectedRuntimeSelectors || []));
  const generatedDamageFamilies = uniqueObjects(factorRows.flatMap((row) => row.affectedGeneratedDamageFamilies || []));
  const generatedOutputFamilies = uniqueObjects(factorRows.flatMap((row) => row.affectedGeneratedOutputFamilies || []));
  const stateRoutes = uniqueObjects(factorRows.flatMap((row) => row.affectedStateRoutes || []));
  const mechanicClasses = classifyMechanics(descriptions.en || "");
  const separateOutputBearing = mechanicClasses.some((kind) => [
    "triggered-damage-output", "summoned-output", "healing-output", "shield-output",
  ].includes(kind));
  const exactStaticOutput = directDamageIds.length > 0
    || exactRecountIds.length > 0
    || exactSkillIds.length > 0
    || runtimeSelectors.length > 0
    || generatedDamageFamilies.length > 0
    || generatedOutputFamilies.length > 0
    || stateRoutes.length > 0;
  const offlineRouteState = exactStaticOutput
    ? "exact-static-output-route"
    : separateOutputBearing
      ? "offline-output-route-needed"
      : "exact-source-state-route-no-separate-output-id";
  const finalValidation = [];
  if (currentRuntimeEligible && offlineRouteState !== "offline-output-route-needed") {
    finalValidation.push("selected-grade-profile-snapshot");
    if (buffIds.length > 0) finalValidation.push("selected-source-buff-lifecycle");
    if ((family.energyBehaviors || []).some((behavior) => behavior === "generate" || behavior === "consume-at-threshold")) {
      finalValidation.push("resource-state-transition");
    }
    if (mechanicClasses.includes("cooldown-manipulation")) finalValidation.push("cooldown-progress-transition");
    if (mechanicClasses.includes("status-property-window")) finalValidation.push("status-lifecycle-and-stacking");
    if (separateOutputBearing) finalValidation.push("output-event-ownership-and-recount-conservation");
  }
  return {
    family_id: Number(family.familyId),
    family_name: family.familyName || "",
    family_names: family.familyNames || {},
    class_gate_ids: numbers(family.classGateIds),
    slot_category: family.slotCategory || null,
    runtime_role: family.runtimeRole || null,
    current_runtime_eligible: currentRuntimeEligible,
    availability: currentRuntimeEligible ? "current-season" : "archived-expired-current-season",
    availability_evidence: currentRuntimeEligible
      ? "no-grade-uses-the-current-client-expiration-description"
      : `all-${gradeRows.length}-grades-use-the-current-client-expiration-description`,
    source_buff_ids: buffIds,
    grade_item_ids: numbers((family.gradeRows || []).flatMap((row) => row.itemId)),
    grade_routes: (family.gradeRows || []).map((row) => ({
      grade: Number(row.grade),
      item_id: Number(row.itemId),
      source_buff_id: Number(row.primaryBuffId),
      parameter_values: numbers(row.parameterValues),
      energy_behavior: row.energy?.behavior || null,
      energy_amount: Number.isFinite(Number(row.energy?.amount)) ? Number(row.energy.amount) : null,
      resolved_description: cleanText(row.energy?.description || ""),
    })),
    energy_behaviors: strings(family.energyBehaviors),
    descriptions,
    mechanic_classes: mechanicClasses,
    direct_damage_ids: directDamageIds,
    declared_recount_ids: declaredRecountIds,
    exact_recount_ids: exactRecountIds,
    exact_skill_ids: exactSkillIds,
    generated_damage_families: generatedDamageFamilies,
    generated_output_families: generatedOutputFamilies,
    state_routes: stateRoutes,
    runtime_selectors: runtimeSelectors,
    localized_recount_name_matches: recountMatches,
    localized_skill_name_matches: skillMatches,
    offline_route_state: offlineRouteState,
    offline_obligation: offlineRouteState === "offline-output-route-needed"
      ? "trace the current-build source buff through exact triggered buff, skill, damage, recount, attribute, resource, cooldown, healing, shield, or summon identifiers"
      : null,
    final_validation_obligations: finalValidation,
  };
}

function isExpiredDescription(value) {
  return cleanText(value) === "This Factor has expired for the current season. It can be recycled at the Recycling Shop.";
}

function familyDescriptions(family, factorRows) {
  const descriptions = {};
  for (const factor of factorRows || []) {
    for (const [locale, value] of Object.entries(factor.cleanDescriptions || factor.descriptions || {})) {
      if (!descriptions[locale] && String(value || "").trim()) descriptions[locale] = cleanText(value);
    }
  }
  for (const row of family.gradeRows || []) {
    const localized = row.energy?.descriptions || row.cleanResolvedDescriptions || row.resolvedDescriptions || {};
    for (const [locale, value] of Object.entries(localized)) {
      if (!descriptions[locale] && String(value || "").trim()) descriptions[locale] = cleanText(value);
    }
    if (!descriptions.en && row.energy?.description) descriptions.en = cleanText(row.energy.description);
  }
  if (!descriptions.en) descriptions.en = cleanText(family.description || "");
  return descriptions;
}

function classifyMechanics(description) {
  const text = String(description || "").toLowerCase();
  const kinds = [];
  if (/triggers?\b|summons?\b|adds?\s+\d+\s+.*(?:wave|attack)|dealing damage summons/.test(text)) kinds.push("triggered-damage-output");
  if (/summons?\b|spawned\b|wild wolf|falcon|speaker|flower stalk/.test(text)) kinds.push("summoned-output");
  if (/heal|restor(?:e|ing) hp|nourish/.test(text)) kinds.push("healing-output");
  if (/shield/.test(text)) kinds.push("shield-output");
  if (/remaining cd|does not trigger cd|cooldown/.test(text)) kinds.push("cooldown-manipulation");
  if (/energy|resource|passion|crystal|sigil|blade intent|charge seed/.test(text)) kinds.push("resource-manipulation");
  if (/final dmg|illusion dmg|attack dmg|class skills final dmg|dmg \+|dmg increases/.test(text)) kinds.push("damage-modifier");
  if (/crit|luck chance|lucky strike/.test(text)) kinds.push("critical-or-lucky-modifier");
  if (/for \d+s|stacking|super armor|max hp|casting spd|dmg reduction|charge/.test(text)) kinds.push("status-property-window");
  return strings(kinds.length ? kinds : ["source-state-mechanic"]);
}

function buildLocaleIndexes(rows, namesOf, fallbackOf) {
  const indexes = new Map();
  for (const row of rows) {
    const names = { ...(namesOf(row) || {}) };
    if (!names.en && fallbackOf(row)) names.en = fallbackOf(row);
    for (const [locale, raw] of Object.entries(names)) {
      const name = cleanText(raw);
      if (!isUsefulName(name)) continue;
      const key = String(locale);
      if (!indexes.has(key)) indexes.set(key, []);
      indexes.get(key).push({ normalized: normalize(name), name, row });
    }
  }
  for (const entries of indexes.values()) entries.sort((a, b) => b.normalized.length - a.normalized.length);
  return indexes;
}

function collectLocalizedMatches(descriptions, indexes, valueOf) {
  const found = new Map();
  for (const [locale, rawDescription] of Object.entries(descriptions || {})) {
    // English is the canonical semantic join surface. Other locales are retained
    // in the artifact for presentation and corroboration, but generic translated
    // terms such as "healing" produced cross-language false joins to unrelated
    // skills and recount rows.
    if (locale !== "en") continue;
    const normalizedDescription = normalize(rawDescription);
    for (const entry of indexes.get(locale) || []) {
      if (!containsName(normalizedDescription, entry.normalized)) continue;
      const value = valueOf(entry.row);
      const key = JSON.stringify(value);
      const current = found.get(key) || { value, evidence: [] };
      current.evidence.push({ locale, matched_text: entry.name });
      found.set(key, current);
    }
  }
  return [...found.values()].map((row) => ({
    ...row,
    evidence: uniqueObjects(row.evidence),
  }));
}

function containsName(description, name) {
  const index = description.indexOf(name);
  if (index < 0) return false;
  if (/^[a-z0-9 ]+$/i.test(name)) {
    const before = index > 0 ? description[index - 1] : " ";
    const after = description[index + name.length] || " ";
    if (/[a-z0-9]/i.test(before) || /[a-z0-9]/i.test(after)) return false;
  }
  return true;
}

function isUsefulName(name) {
  const compact = normalize(name);
  if (compact.length < 4) return false;
  if (/^(skill|attack|ultimate|special attack|damage|healing|healing strength|shield|buff|effect|critical hit|crit|lucky strike)$/i.test(compact)) return false;
  return true;
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) continue;
    result[token.slice(2)] = args[index + 1];
    index += 1;
  }
  return result;
}

function requiredPath(value, flag) {
  if (!value) throw new Error(`${flag} is required`);
  return path.resolve(value);
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function cleanText(value) {
  return String(value || "")
    .replace(/<[^>]*>/g, " ")
    .replace(/\{\*[^}]+\*\}/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function normalize(value) {
  return cleanText(value).normalize("NFKC").toLocaleLowerCase().replace(/[’']/g, "'");
}

function numbers(values) {
  return [...new Set((Array.isArray(values) ? values : [values])
    .map(Number)
    .filter((value) => Number.isFinite(value) && value > 0))]
    .sort((a, b) => a - b);
}

function strings(values) {
  return [...new Set((Array.isArray(values) ? values : [values])
    .map((value) => String(value || "").trim())
    .filter(Boolean))]
    .sort();
}

function uniqueObjects(values) {
  const rows = new Map();
  for (const value of values || []) rows.set(JSON.stringify(value), value);
  return [...rows.values()];
}

function countBy(values, keyOf) {
  const result = {};
  for (const value of values) {
    const key = keyOf(value);
    result[key] = (result[key] || 0) + 1;
  }
  return sortObject(result);
}

function sortObject(value) {
  return Object.fromEntries(Object.entries(value).sort(([left], [right]) => left.localeCompare(right)));
}

function normalizePath(value) {
  return path.resolve(value).replaceAll("\\", "/");
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
