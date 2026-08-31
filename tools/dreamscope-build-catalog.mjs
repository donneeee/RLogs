#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDirectory, "..");
const workspaceRoot = path.resolve(repositoryRoot, "..");
const options = parseArgs(process.argv.slice(2));
const gameBuild = String(options.build || "").trim();
if (!/^\d+$/.test(gameBuild)) {
  throw new Error("--build must be the numeric client build");
}

const excelsDirectory = path.resolve(options.excels || path.join(repositoryRoot, "Excels"));
const factorSourcePath = path.resolve(options.factors || path.join(
  workspaceRoot,
  "BPSR-UID-Extractors",
  `output-build-${gameBuild}-exact`,
  "SeasonPhantomFactors.json",
));
const formulaRuntimePath = path.resolve(options.formulas || path.join(
  repositoryRoot,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-formula-runtime.v1.json",
));
const modifierFormulaPath = path.resolve(options["modifier-formulas"] || path.join(
  workspaceRoot,
  "BPSR-UID-Extractors",
  `output-build-${gameBuild}-exact`,
  "ModifierFormulaTermTable.json",
));
const outputPath = path.resolve(options.output || path.join(
  repositoryRoot,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/dreamscope-build-inference.v1.json",
));

const templates = rows(readJson(path.join(excelsDirectory, "SeasonTalentTemplateTable.json")));
const treeRows = rows(readJson(path.join(excelsDirectory, "SeasonTalentTreeTable.json")));
const ordinaryEffects = rows(readJson(path.join(excelsDirectory, "SeasonTalentEffectOrdinaryTable.json")));
const advancedEffects = rows(readJson(path.join(excelsDirectory, "SeasonTalentEffectAdvancedTable.json")));
const factorHoles = rows(readJson(path.join(excelsDirectory, "SeasonTalentAdvancedHoleTable.json")));
const factorItemRows = rows(readJson(path.join(excelsDirectory, "SeasonTalentFactorItemTable.json")));
const factorTypes = rows(readJson(path.join(excelsDirectory, "SeasonTalentFactorTypeTable.json")));
const factors = readJson(factorSourcePath);
const formulaRuntime = readJson(formulaRuntimePath);
const modifierFormulaEntries = readJson(modifierFormulaPath).entriesByKey || {};

const ordinaryByNodeId = indexBy(ordinaryEffects, (row) => number(row.Id));
const factorTypeById = new Map(factorTypes.map((row) => [number(row.Id), String(row.Name || "")]));
const factorItemTableById = indexBy(factorItemRows, (row) => number(row.Id));

const templatesById = {};
for (const row of templates.sort(byNumericId)) {
  const id = number(row.Id);
  templatesById[id] = {
    id,
    name: String(row.TemplateName || ""),
    season_id: numberOrNull(row.BelongSeasonId),
    function_id: numberOrNull(row.BelongFunction),
    root_node_id: numberOrNull(row.NoteRootId),
    advanced_effect_id: numberOrNull(row.AdvancedEffectId),
    selectable_node_ids: treeRows
      .filter((node) => number(node.TemplateId) === id && number(node.NodeType) === 1)
      .map((node) => number(node.Id))
      .sort(ascending),
  };
}

const treeNodesById = {};
const candidatesByTerminalEffectId = {};
for (const row of treeRows.sort(byNumericId)) {
  const nodeId = number(row.Id);
  const effect = ordinaryByNodeId.get(nodeId);
  const terminalEffectIds = effectIds(effect?.Effect);
  const node = {
    node_id: nodeId,
    template_id: number(row.TemplateId),
    name: String(effect?.Name || ""),
    node_type: number(row.NodeType),
    group_id: number(row.GroupId),
    pre_node_ids: numbers(row.PreNode),
    next_node_ids: numbers(row.NextNode),
    mutually_exclusive_node_id: positiveNumberOrNull(row.SameNodeGroupId),
    terminal_effect_ids: terminalEffectIds,
    icon_path: String(effect?.IconNormal || ""),
  };
  treeNodesById[nodeId] = node;
  if (node.node_type !== 1) continue;
  for (const effectId of terminalEffectIds) {
    addCandidate(candidatesByTerminalEffectId, effectId, {
      source_kind: "tree_node",
      source_id: nodeId,
      name: node.name,
      template_id: node.template_id,
      item_ids: [],
      grades: [],
    });
  }
}

const advancedEffectsById = {};
for (const row of advancedEffects.sort(byNumericId)) {
  const id = number(row.Id);
  const terminalEffectIds = effectIds(row.Effect);
  const advanced = {
    id,
    advanced_effect_id: number(row.EffectId),
    level: number(row.Level),
    name: String(row.Name || ""),
    terminal_effect_ids: terminalEffectIds,
    icon_path: String(row.Icon || ""),
  };
  advancedEffectsById[id] = advanced;
  for (const effectId of terminalEffectIds) {
    addCandidate(candidatesByTerminalEffectId, effectId, {
      source_kind: "advanced_tree_effect",
      source_id: id,
      name: advanced.name,
      template_id: null,
      item_ids: [],
      grades: [advanced.level],
    });
  }
}

const factorSlotsByTemplateId = {};
for (const row of factorHoles.sort(byNumericId)) {
  const templateId = number(row.TempId);
  (factorSlotsByTemplateId[templateId] ||= []).push({
    row_id: number(row.Id),
    hole_id: number(row.HoleId),
    name: String(row.Name || ""),
  });
}

const factorFamiliesById = {};
const factorItemsById = {};
const candidatesByDamageId = {};
const candidatesByRecountId = {};
for (const sourceFamily of Object.values(factors.factorFamiliesById || {}).sort((a, b) => number(a.familyId) - number(b.familyId))) {
  const familyId = number(sourceFamily.familyId);
  const gradeItems = (sourceFamily.gradeRows || [])
    .map((gradeRow) => {
      const itemId = number(gradeRow.itemId);
      const tableRow = factorItemTableById.get(itemId);
      const terminalEffectIds = positiveNumbers([
        gradeRow.primaryBuffId,
        ...effectIds(tableRow?.FactorItemEffect),
      ]);
      const item = {
        item_id: itemId,
        family_id: familyId,
        grade: number(gradeRow.grade),
        quality_tier: numberOrNull(gradeRow.itemQualityTier),
        factor_type_id: numberOrNull(tableRow?.FactorItemTypeId),
        factor_type: factorTypeById.get(number(tableRow?.FactorItemTypeId)) || String(sourceFamily.slotCategory || ""),
        season_ids: numbers(tableRow?.SeasonId),
        profession_ids: numbers(tableRow?.ProfessionId),
        terminal_effect_ids: terminalEffectIds,
      };
      factorItemsById[itemId] = item;
      return item;
    })
    .sort((a, b) => a.grade - b.grade || a.item_id - b.item_id);

  const terminalEffectIds = uniqueNumbers(gradeItems.flatMap((item) => item.terminal_effect_ids));
  const family = {
    family_id: familyId,
    name: String(sourceFamily.familyName || ""),
    names: sourceFamily.familyNames || {},
    slot_category: String(sourceFamily.slotCategory || ""),
    runtime_role: String(sourceFamily.runtimeRole || ""),
    class_gate_ids: numbers(sourceFamily.classGateIds),
    terminal_effect_ids: terminalEffectIds,
    direct_damage_ids: numbers(sourceFamily.affectedDamageIds),
    recount_ids: numbers(sourceFamily.affectedRecountIds),
    item_ids: gradeItems.map((item) => item.item_id),
  };
  factorFamiliesById[familyId] = family;

  for (const effectId of terminalEffectIds) {
    const matchingItems = gradeItems.filter((item) => item.terminal_effect_ids.includes(effectId));
    addCandidate(candidatesByTerminalEffectId, effectId, {
      source_kind: "factor_family",
      source_id: familyId,
      name: family.name,
      template_id: null,
      item_ids: matchingItems.map((item) => item.item_id),
      grades: matchingItems.map((item) => item.grade),
    });
  }
  for (const damageId of family.direct_damage_ids) {
    addCandidate(candidatesByDamageId, damageId, factorCandidate(family));
  }
  for (const recountId of family.recount_ids) {
    addCandidate(candidatesByRecountId, recountId, factorCandidate(family));
  }
}

// The extractor also exposes exact reverse edges that may span multiple factor
// families. Keep them even when the compact family row has no direct list.
for (const [damageId, effectIds] of Object.entries(factors.damageIdToFactorBuffIds || {})) {
  for (const effectId of numbers(effectIds)) {
    for (const candidate of candidatesByTerminalEffectId[String(effectId)] || []) {
      if (candidate.source_kind === "factor_family") addCandidate(candidatesByDamageId, number(damageId), candidate);
    }
  }
}
for (const [recountId, effectIds] of Object.entries(factors.recountIdToFactorBuffIds || {})) {
  for (const effectId of numbers(effectIds)) {
    for (const candidate of candidatesByTerminalEffectId[String(effectId)] || []) {
      if (candidate.source_kind === "factor_family") addCandidate(candidatesByRecountId, number(recountId), candidate);
    }
  }
}

// Some selected nodes terminate at a hidden source/config effect while the
// packet emits a distinct visible runtime effect. These bridges must be
// declared by an audited runtime formula and prove the same current-build
// formula component and scalar. Never infer a bridge from neighboring IDs.
const candidatesByRuntimeEffectId = {};
const runtimeEffectLinksById = {};
for (const [formulaKey, config] of Object.entries(formulaRuntime)) {
  if (!config || typeof config !== "object" || Array.isArray(config)) continue;
  const runtimeEffectId = positiveNumberOrNull(config.effect_id);
  const sourceTerminalEffectId = positiveNumberOrNull(config.source_terminal_effect_id);
  if (runtimeEffectId === null || sourceTerminalEffectId === null) continue;

  const sourceCandidates = candidatesByTerminalEffectId[String(sourceTerminalEffectId)] || [];
  if (sourceCandidates.length === 0) {
    throw new Error(`${formulaKey}: source terminal effect ${sourceTerminalEffectId} has no current-build Dreamscope source`);
  }
  const sourceFormula = modifierFormulaEntries[`buffs:${sourceTerminalEffectId}`];
  const runtimeFormula = modifierFormulaEntries[`buffs:${runtimeEffectId}`];
  const formulaMatches = matchingFormulaComponents(sourceFormula, runtimeFormula);
  if (formulaMatches.length === 0) {
    throw new Error(`${formulaKey}: ${sourceTerminalEffectId} -> ${runtimeEffectId} has no identical current-build formula component/value`);
  }
  const configuredPercentRawDelta = numberOrNull(config.primary_percent_raw_delta);
  if (configuredPercentRawDelta !== null && !formulaMatches.some(
    (row) => Math.round(row.decimal_value * 10_000) === configuredPercentRawDelta,
  )) {
    throw new Error(`${formulaKey}: configured raw percent ${configuredPercentRawDelta} does not match the current-build formula scalar`);
  }

  runtimeEffectLinksById[runtimeEffectId] = {
    runtime_effect_id: runtimeEffectId,
    source_terminal_effect_id: sourceTerminalEffectId,
    formula_key: formulaKey,
    proof_state: "exact_current_build_formula_match",
    formula_matches: formulaMatches,
    configured_percent_raw_delta: configuredPercentRawDelta,
    source_scope_kinds: [...(sourceFormula?.scopeKinds || [])].sort(),
    runtime_scope_kinds: [...(runtimeFormula?.scopeKinds || [])].sort(),
  };
  for (const candidate of sourceCandidates) {
    addCandidate(candidatesByRuntimeEffectId, runtimeEffectId, candidate);
  }
}

sortCandidateIndex(candidatesByTerminalEffectId);
sortCandidateIndex(candidatesByRuntimeEffectId);
sortCandidateIndex(candidatesByDamageId);
sortCandidateIndex(candidatesByRecountId);

const catalog = {
  schema_version: 1,
  game: "blue-protocol-star-resonance",
  game_build: gameBuild,
  generated_by: "tools/dreamscope-build-catalog.mjs",
  policy: {
    tree_nodes_and_factor_slots_are_separate_systems: true,
    packet_terminal_ids_are_runtime_authority: true,
    runtime_effect_bridges_require_explicit_formula_and_current_build_scalar_match: true,
    adjacent_ids_never_imply_a_runtime_effect_bridge: true,
    duplicate_terminal_ids_remain_ambiguous: true,
    absent_runtime_evidence_does_not_prove_absence: true,
    r_dps_requires_provider_recipient_window_proof: true,
  },
  summary: {
    templates: Object.keys(templatesById).length,
    selectable_tree_nodes: Object.values(treeNodesById).filter((row) => row.node_type === 1).length,
    advanced_tree_effects: Object.keys(advancedEffectsById).length,
    factor_slot_templates: Object.keys(factorSlotsByTemplateId).length,
    factor_slots: Object.values(factorSlotsByTemplateId).reduce((total, slots) => total + slots.length, 0),
    factor_families: Object.keys(factorFamiliesById).length,
    factor_items: Object.keys(factorItemsById).length,
    terminal_effect_ids: Object.keys(candidatesByTerminalEffectId).length,
    ambiguous_terminal_effect_ids: Object.values(candidatesByTerminalEffectId).filter((rows) => rows.length > 1).length,
    runtime_effect_ids: Object.keys(candidatesByRuntimeEffectId).length,
    ambiguous_runtime_effect_ids: Object.values(candidatesByRuntimeEffectId).filter((rows) => rows.length > 1).length,
  },
  inputs: {
    excels_directory: normalizePath(excelsDirectory),
    factors: normalizePath(factorSourcePath),
    formulas: normalizePath(formulaRuntimePath),
    modifier_formulas: normalizePath(modifierFormulaPath),
  },
  templates_by_id: templatesById,
  tree_nodes_by_id: treeNodesById,
  advanced_effects_by_id: advancedEffectsById,
  factor_slots_by_template_id: factorSlotsByTemplateId,
  factor_families_by_id: factorFamiliesById,
  factor_items_by_id: factorItemsById,
  candidates_by_terminal_effect_id: candidatesByTerminalEffectId,
  candidates_by_runtime_effect_id: candidatesByRuntimeEffectId,
  runtime_effect_links_by_id: runtimeEffectLinksById,
  candidates_by_damage_id: candidatesByDamageId,
  candidates_by_recount_id: candidatesByRecountId,
};

writeFileSync(outputPath, `${JSON.stringify(catalog, null, 2)}\n`, "utf8");
console.log(JSON.stringify({ output: normalizePath(outputPath), ...catalog.summary }));

function factorCandidate(family) {
  return {
    source_kind: "factor_family",
    source_id: family.family_id,
    name: family.name,
    template_id: null,
    item_ids: [...family.item_ids],
    grades: family.item_ids.map((itemId) => factorItemsById[itemId]?.grade).filter(Number.isFinite),
  };
}

function addCandidate(index, id, candidate) {
  if (!Number.isFinite(Number(id))) return;
  const key = String(number(id));
  const list = (index[key] ||= []);
  const signature = `${candidate.source_kind}:${candidate.source_id}`;
  const current = list.find((row) => `${row.source_kind}:${row.source_id}` === signature);
  if (current) {
    current.item_ids = uniqueNumbers([...current.item_ids, ...candidate.item_ids]);
    current.grades = uniqueNumbers([...current.grades, ...candidate.grades]);
    return;
  }
  list.push({
    ...candidate,
    item_ids: uniqueNumbers(candidate.item_ids),
    grades: uniqueNumbers(candidate.grades),
  });
}

function sortCandidateIndex(index) {
  for (const rows of Object.values(index)) {
    rows.sort((left, right) => left.source_kind.localeCompare(right.source_kind) || left.source_id - right.source_id);
  }
}

function effectIds(value) {
  if (!Array.isArray(value)) return [];
  return positiveNumbers(value.flatMap((entry) => Array.isArray(entry) && entry.length > 1 ? [entry[1]] : []));
}

function matchingFormulaComponents(source, runtime) {
  const sourceRows = formulaComponents(source);
  const runtimeRows = new Map(formulaComponents(runtime).map((row) => [row.signature, row]));
  return sourceRows
    .filter((row) => runtimeRows.has(row.signature))
    .map(({ signature: _, ...row }) => row)
    .sort((left, right) => left.component_key.localeCompare(right.component_key) || left.decimal_value - right.decimal_value);
}

function formulaComponents(entry) {
  return (entry?.componentValueHints || []).flatMap((component) =>
    (component.values || []).map((value) => {
      const componentKey = String(component.componentKey || "");
      const unit = String(value.unit || "");
      const decimalValue = Number(value.decimalValue);
      const signature = `${componentKey}|${unit}|${decimalValue}`;
      return {
        signature,
        component_key: componentKey,
        unit,
        decimal_value: decimalValue,
        source_scope: String(value.scope || component.valueScope || ""),
      };
    }).filter((row) => row.component_key && Number.isFinite(row.decimal_value)),
  );
}

function rows(value) {
  return (Array.isArray(value) ? value : Object.values(value || {})).filter((row) => row && typeof row === "object");
}

function indexBy(values, selector) {
  return new Map(values.map((row) => [selector(row), row]));
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function numbers(value) {
  const values = Array.isArray(value) ? value.flat(Infinity) : [value];
  return uniqueNumbers(values);
}

function uniqueNumbers(values) {
  return [...new Set(values.map(Number).filter(Number.isFinite))].sort(ascending);
}

function positiveNumbers(values) {
  return uniqueNumbers(values).filter((value) => value > 0);
}

function number(value) {
  return Number(value);
}

function numberOrNull(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function positiveNumberOrNull(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

function ascending(left, right) {
  return left - right;
}

function byNumericId(left, right) {
  return number(left.Id) - number(right.Id);
}

function normalizePath(value) {
  return path.resolve(value).replaceAll("\\", "/");
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`unexpected argument ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`${token} requires a value`);
    result[key] = value;
    index += 1;
  }
  return result;
}
