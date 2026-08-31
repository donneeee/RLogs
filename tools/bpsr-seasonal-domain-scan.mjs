#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const pluginRoot = path.join(
  repoRoot,
  "plugins",
  "games",
  "blue-protocol-star-resonance",
);

const domains = [
  domain("skills", [
    decoded("SkillDataTable.json"), decoded("SkillEffectTable.json"),
    decoded("SkillFightLevelTable.json"), decoded("SkillSystemTable.json"),
    decoded("SkillTable.json"), decoded("SkillUpgradeTable.json"),
    extracted("SkillDescriptions.json", ["entriesByUid"]),
  ], ["Rebuild added and changed skill identities without removing packet-observed IDs.", "Rebuild skill-to-effect, damage-chain, and recount relationships.", "Re-run formula, lifecycle, recipient, and conservation proofs."], ["combat-table-diff", "origin-graph-diff", "formula-stage-replay"]),
  domain("talents", [
    decoded("TalentPoolTable.json"), decoded("TalentSchoolTable.json"),
    decoded("TalentStageTable.json"), decoded("TalentTable.json"),
    decoded("TalentTagTable.json"), decoded("TalentTreeTable.json"),
    decoded("SeasonTalentAdvancedHoleTable.json"),
    decoded("SeasonTalentEffectAdvancedTable.json"),
    decoded("SeasonTalentEffectIntermediateTable.json"),
    decoded("SeasonTalentEffectOrdinaryTable.json"),
    decoded("SeasonTalentTemplateTable.json"), decoded("SeasonTalentTreeTable.json"),
    extracted("TalentDescriptions.json", ["entriesByUid"]),
    extracted("TalentSpecOwnership.json", ["entriesByUid"], false),
    extracted("SeasonalTalentDescriptions.json", ["entriesByUid"], false),
  ], ["Identify new, removed, and rescaled class and seasonal talents.", "Rebuild talent ownership, proc, effect, and recount edges.", "Re-run specialization, formula, provider, recipient, and conservation proofs."], ["combat-table-diff", "origin-graph-diff", "provider-recipient-replay"]),
  domain("imagines", [
    decoded("SkillAoyiGuideEffectTable.json"), decoded("SkillAoyiGuideTable.json"),
    decoded("SkillAoyiItemTable.json"), decoded("SkillAoyiStarTable.json"),
    decoded("SkillAoyiTable.json"), decoded("SkillAoyiTransformTable.json"),
    extracted("BattleImagineDescriptions.json", ["entriesByUid"]),
    extracted("skill_aoyi_icons.json", ["$"], false, true),
  ], ["Add every newly introduced Imagine UID and retain every removed UID as migration evidence.", "Diff every existing Imagine tier, star, remodel, and seasonal scaling row before reuse.", "Rebuild Imagine ownership, summon, trigger, buff, damage-chain, recipient-scope, and recount edges.", "Refresh icon and localization references without embedding localized strings in mechanics data.", "Re-run formula, provider, recipient, lifecycle, and conservation proofs."], ["combat-table-diff", "origin-graph-diff", "formula-stage-replay", "provider-recipient-replay"]),
  domain("psychoscope-factors", [
    decoded("RogueBuffTable.json"), decoded("RogueEntryTable.json"), decoded("RogueTable.json"),
    decoded("SeasonTalentFactorItemTable.json"), decoded("SeasonTalentFactorTypeTable.json"),
    extracted("SeasonPhantomFactors.json", ["factorBuffIds", "factorsByBuffId", "directAttributeFactorsByFamilyId", "directAttributeItemToFamilyId", "factorFamiliesById", "factorItemsById", "damageIdToFactorBuffIds", "recountIdToFactorBuffIds", "runtimeSelectorFactors"]),
    extracted("FactorDescriptions.json", ["entriesByUid"], false),
  ], ["Diff Polarity, Stasis, Inspiration, and Reality factor identities and scaling.", "Rebuild energy generation, consumption, skill mutation, buff, triggered attack, and recount edges.", "Re-run factor lifecycle, stacking, formula, provider, recipient, and conservation proofs."], ["factor-event-correlation", "origin-graph-diff", "provider-recipient-replay"]),
  domain("equipment-set-bonuses", [
    decoded("EquipSuitTable.json"), decoded("EquipTable.json"), decoded("EquipWeaponTable.json"),
    extracted("EquipmentSetEffects.json", ["families", "set_named_buffs"]),
  ], ["Diff equipment and set-bonus identities, thresholds, effects, and scaling.", "Rebuild set-to-buff, triggered damage, skill mutation, and recount edges.", "Re-run formula, lifecycle, provider, recipient, and conservation proofs."], ["combat-table-diff", "origin-graph-diff", "status-lifecycle-replay"]),
  domain("buffs-effects", [
    decoded("BuffDataTable.json"), decoded("BuffTable.json"), decoded("DamageAttrTable.json"),
    extracted("BuffDescriptions.json", ["entriesByUid"]),
    extracted("BuffName.json", ["$"], false),
    extracted("EffectSources.json", ["entriesByUid"], false),
    extracted("SeasonEffectDescriptions.json", ["entriesByUid"], false),
  ], ["Retain and classify every added, removed, or changed buff, effect, and damage row.", "Rebuild exact origin, lifecycle, stacking, recipient-scope, and damage-chain edges.", "Keep unresolved rows visible and disabled for attribution until proven."], ["combat-table-diff", "status-lifecycle-replay", "provider-recipient-replay"]),
  domain("formulas-scaling", [
    decoded("DamageAttrTable.json"), decoded("FightAttrTable.json"), decoded("FightAttrTranTable.json"),
    extracted("DamageFormulaSurface.json", ["rows", "linked_hit_event_candidate_lookup"], false),
    extracted("FightAttributeTransform.json", ["rows", "referenced_pool_records"], false),
    extracted("ModifierFormulaTermRuntime.json", ["entriesByKey"], false),
    extracted("ModifierFormulaTermTable.json", ["entriesByKey"], false),
    extracted("ModifierValueProofRuntime.json", ["entriesByKey"], false),
    extracted("ModifierValueProofTable.json", ["entriesByKey"], false),
  ], ["Diff all coefficient, unit, percentage, curve, cap, HP-scaling, mitigation, and rounding inputs.", "Invalidate only proof families whose exact input rows changed.", "Re-run stage-order, formula, attribute-state, and conservation proofs."], ["formula-surface-diff", "formula-stage-replay", "attribute-state-replay"]),
  domain("relationships-recount", [
    decoded("RecountTable.json", false),
    extracted("RecountTable.json", ["$"]),
    extracted("ModifierRelationshipTable.json", ["sourcesByRuleId"], false),
    extracted("ModifierRecountTable.json", ["sourcesById"], false),
    extracted("ModifierClassificationRuntime.json", ["sourcesByRuleId"], false),
    extracted("ModifierClassificationTable.json", ["sourcesByRuleId"], false),
    extracted("ModifierContributionRuntime.json", ["sourcesByRuleId"], false),
    extracted("ModifierContributionTable.json", ["sourcesByRuleId"], false),
    extracted("SkillDamageChainBridge.json", ["recountChains", "damageChains"], false),
    extracted("SkillDamageChainRuntime.json", ["recountChains", "damageChains"]),
  ], ["Diff ownership, child-to-parent recount, provider, recipient, and contribution edges.", "Preserve all child events while rebuilding parent totals.", "Re-run exact party and skill conservation proofs."], ["origin-graph-diff", "provider-recipient-replay", "canonical-replay-conservation"]),
  domain("seasonal-activity-identity", [
    decoded("SeasonActTable.json"), decoded("SeasonActTargetTable.json"),
  ], ["Diff season and activity identities used to scope proof evidence.", "Rebuild scene-season routing without parsing localized titles."], ["game-file-schema-diff", "canonical-replay-conservation"]),
  domain("scenes-encounters-entities", [
    decoded("SceneTable.json"), decoded("SubSceneTable.json"),
    decoded("SceneAreaTable.json"), decoded("SceneTagTable.json"),
    decoded("SceneTagShowTable.json"), decoded("SceneVariablesTable.json"),
    decoded("SceneResourceTable.json"), decoded("NeighbouringSceneTable.json"),
    decoded("DungeonsTable.json"), decoded("DungeonStageTable.json"),
    decoded("ActivityDungeonTable.json"), decoded("MainPlotDungeonTable.json"),
    decoded("NormalHeroDungeonTable.json"), decoded("MasterChallengeDungeonTable.json"),
    decoded("RaidDungeonTable.json"), decoded("RaidBossTable.json"),
    decoded("MonsterTable.json"), decoded("MonsterEntityTable.json"),
    decoded("MonsterEntityGlobalTable.json"), decoded("MonsterFightAreaTable.json"),
    decoded("NpcTable.json"), decoded("NpcEntityTable.json"),
    decoded("NpcEntityGlobalTable.json"), decoded("DummyEntityTable.json"),
    decoded("BattlePetTable.json"), decoded("PetTable.json"),
    decoded("SceneObjectTable.json"), decoded("SceneObjectEntityTable.json"),
    decoded("SceneObjectEntityGlobalTable.json"), decoded("ZoneEntityTable.json"),
    decoded("ZoneEntityGlobalTable.json"), decoded("RandomEntityTable.json"),
    decoded("TrapEntityTable.json"), decoded("VehicleEntityTable.json"),
    decoded("BulletShapeTable.json"),
    extracted("scenenames.json", ["$"], false, true),
    extracted("monsternames.json", ["$"], false, true),
  ], ["Diff scene, dungeon, raid, boss, monster, NPC, pet, projectile, and object identities.", "Rebuild scene boundaries, encounter classification, entity ownership, target filtering, and boss selection from exact tables.", "Keep encounter cardinality data-driven; never cap bosses per scene from prior observations."], ["game-file-schema-diff", "scene-boundary-replay", "entity-ownership-replay", "target-classification-replay"]),
  domain("classes-specializations-loadouts", [
    decoded("ProfessionTable.json"), decoded("ProfessionSystemTable.json"),
    decoded("PlayerLevelSkillTable.json"), decoded("SkillSlotPositionTable.json"),
    decoded("SpecialSkillSlotTable.json"), decoded("SkillDutyTable.json"),
    decoded("SkillLabelTable.json"), decoded("RoleCardTable.json"),
    extracted("class-labels.json", ["$"], false, true),
    extracted("class-spec-icons.json", ["$"], false, true),
    extracted("spec-icons.json", ["$"], false, true),
    extracted("TalentSpecOwnership.json", ["entriesByUid"], false),
  ], ["Diff class, specialization, role, skill-slot, and loadout identities.", "Rebuild packet-observed class and specialization evidence without applying current local snapshots retroactively.", "Refresh class and specialization icon references separately from mechanics authority."], ["class-spec-evidence-replay", "loadout-snapshot-replay", "profile-snapshot-replay"]),
  domain("items-weapons-profile", [
    decoded("EquipTable.json"), decoded("EquipWeaponTable.json"),
    decoded("WeaponAttrTable.json"), decoded("WeaponForgeTable.json"),
    decoded("WeaponLevelTable.json"), decoded("WeaponStarTable.json"),
    decoded("EquipBreakThroughTable.json"), decoded("EquipCreateTable.json"),
    decoded("EquipPartTable.json"), decoded("EquipSuitTable.json"),
    decoded("EquipTransformTable.json"), decoded("ItemTypeTable.json"),
    extracted("ItemDescriptions.json", ["entriesByUid"], false, true),
    extracted("ItemDescriptionSources.json", ["$"], false, true),
    extracted("itemnames.json", ["$"], false, true),
  ], ["Diff non-skin weapon, equipment, item, level, star, forge, and set identities.", "Rebuild exact equipped-weapon and profile snapshot mappings from packet evidence.", "Do not use weapon-skin rows as equipped weapon identity."], ["equipment-profile-replay", "weapon-identity-replay", "origin-graph-diff"]),
  domain("localization-presentation-references", [
    extracted("SkillDescriptions.json", ["entriesByUid"], false, true),
    extracted("TalentDescriptions.json", ["entriesByUid"], false, true),
    extracted("SeasonalTalentDescriptions.json", ["entriesByUid"], false, true),
    extracted("BattleImagineDescriptions.json", ["entriesByUid"], false, true),
    extracted("BuffDescriptions.json", ["entriesByUid"], false, true),
    extracted("SeasonEffectDescriptions.json", ["entriesByUid"], false, true),
    extracted("FactorDescriptions.json", ["entriesByUid"], false, true),
    extracted("ModifierDescriptions.json", ["$"], false, true),
    extracted("ModifierDescriptionCatalogs.json", ["$"], false, true),
    extracted("ModifierDisplayTable.json", ["$"], false, true),
    extracted("skill_aoyi_icons.json", ["$"], false, true),
    extracted("class-spec-icons.json", ["$"], false, true),
    extracted("spec-icons.json", ["$"], false, true),
  ], ["Diff localization keys, labels, descriptions, and icon references independently of mechanics.", "Regenerate each language plug-in without embedding localized text in core or mechanics data.", "Fall back to stable IDs when a presentation reference remains absent."], ["localization-bundle-diff", "presentation-reference-audit"]),
];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "scan") scan(options);
else if (command === "diff") diff(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function scan(options) {
  const build = required(options, "build");
  const extractorRoot = resolvePath(options.extractorRoot || path.join("..", "BPSR-UID-Extractors", `output-build-${build}`));
  const decodedRoot = resolvePath(options.decodedRoot || path.join("..", ".codex_tmp", `current-build-${build}-table-extract-candidate`, "Excels"));
  const outputRoot = resolvePath(options.outputRoot || path.join(pluginRoot, "research", "game-file-inventory", "global", `steam-${build}`, "seasonal-domains"));
  mkdirSync(outputRoot, { recursive: true });
  const summary = { schemaVersion: 1, generatedBy: "tools/bpsr-seasonal-domain-scan.mjs", game: "blue-protocol-star-resonance", deployment: "global", channel: "steam", gameBuild: String(build), policy: policy(), domains: [], missingRequiredInputs: [], missingOptionalInputs: [] };
  for (const definition of domains) {
    const manifest = buildManifest(definition, build, extractorRoot, decodedRoot);
    const output = path.join(outputRoot, `${definition.id}.v1.json`);
    writeJson(output, manifest);
    summary.domains.push({ domain: definition.id, path: relativeRepo(output), aggregateSha256: manifest.aggregateSha256, sourceCount: manifest.sources.length, rowCount: manifest.summary.rowCount, missingRequiredCount: manifest.summary.missingRequiredCount, missingOptionalCount: manifest.summary.missingOptionalCount });
    summary.missingRequiredInputs.push(...manifest.missingRequiredInputs.map((value) => `${definition.id}:${value}`));
    summary.missingOptionalInputs.push(...manifest.missingOptionalInputs.map((value) => `${definition.id}:${value}`));
  }
  const summaryPath = path.join(outputRoot, "index.v1.json");
  writeJson(summaryPath, summary);
  console.log(`Wrote ${summary.domains.length} domain manifests to ${relativeRepo(outputRoot)}`);
  console.log(`Rows fingerprinted: ${summary.domains.reduce((sum, item) => sum + item.rowCount, 0)}`);
  console.log(`Missing required inputs: ${summary.missingRequiredInputs.length}; optional: ${summary.missingOptionalInputs.length}`);
  if (summary.missingRequiredInputs.length) process.exitCode = 2;
}

function diff(options) {
  const baselineBuild = required(options, "baselineBuild");
  const candidateBuild = required(options, "candidateBuild");
  const inventoryRoot = resolvePath(options.inventoryRoot || path.join(pluginRoot, "research", "game-file-inventory", "global"));
  const baselineRoot = path.join(inventoryRoot, `steam-${baselineBuild}`, "seasonal-domains");
  const candidateRoot = path.join(inventoryRoot, `steam-${candidateBuild}`, "seasonal-domains");
  const output = { schemaVersion: 1, generatedBy: "tools/bpsr-seasonal-domain-scan.mjs", game: "blue-protocol-star-resonance", deployment: "global", channel: "steam", baselineBuild: String(baselineBuild), candidateBuild: String(candidateBuild), policy: policy(), changedDomains: [], unchangedDomains: [], missingManifests: [] };
  for (const definition of domains) {
    const baselinePath = path.join(baselineRoot, `${definition.id}.v1.json`);
    const candidatePath = path.join(candidateRoot, `${definition.id}.v1.json`);
    if (!existsSync(baselinePath) || !existsSync(candidatePath)) { output.missingManifests.push({ domain: definition.id, baseline: existsSync(baselinePath), candidate: existsSync(candidatePath) }); continue; }
    const baseline = readJson(baselinePath);
    const candidate = readJson(candidatePath);
    const change = compareManifests(definition, baseline, candidate);
    if (change.addedRows.length || change.removedRows.length || change.changedRows.length || change.addedSources.length || change.removedSources.length) output.changedDomains.push(change);
    else output.unchangedDomains.push(definition.id);
  }
  const outputPath = resolvePath(options.output || path.join(candidateRoot, `diff-from-${baselineBuild}.v1.json`));
  writeJson(outputPath, output);
  console.log(`Changed domains: ${output.changedDomains.length}; unchanged: ${output.unchangedDomains.length}; missing: ${output.missingManifests.length}`);
  for (const item of output.changedDomains) console.log(`${item.domain}: +${item.addedRows.length} -${item.removedRows.length} ~${item.changedRows.length}`);
  if (output.missingManifests.length) process.exitCode = 2;
}

function buildManifest(definition, build, extractorRoot, decodedRoot) {
  const manifest = { schemaVersion: 1, generatedBy: "tools/bpsr-seasonal-domain-scan.mjs", game: "blue-protocol-star-resonance", deployment: "global", channel: "steam", gameBuild: String(build), domain: definition.id, policy: policy(), proofSuites: definition.proofSuites, changeActions: definition.actions, roots: { extractor: "private-extractor-output", decoded: "private-decoded-table-root" }, sources: [], missingRequiredInputs: [], missingOptionalInputs: [], summary: { sourceCount: 0, rowCount: 0, missingRequiredCount: 0, missingOptionalCount: 0 }, aggregateSha256: "" };
  for (const input of definition.inputs) {
    const root = input.root === "decoded" ? decodedRoot : extractorRoot;
    const file = path.join(root, input.file);
    if (!existsSync(file)) { (input.required ? manifest.missingRequiredInputs : manifest.missingOptionalInputs).push(`${input.root}:${input.file}`); continue; }
    const raw = readFileSync(file);
    const value = JSON.parse(raw.toString("utf8"));
    const fingerprints = {};
    const collectionCounts = {};
    for (const collectionPath of input.collections) {
      const collection = resolveCollection(value, collectionPath);
      if (collection === undefined) continue;
      let count = 0;
      for (const [identity, row] of enumerateRows(collection)) { fingerprints[`${collectionPath}:${identity}`] = hash(canonical(sanitizeSemantic(row))); count += 1; }
      collectionCounts[collectionPath] = count;
    }
    const semanticSha256 = hash(Object.entries(fingerprints).sort(([a], [b]) => a.localeCompare(b)).map(([key, value]) => `${key}:${value}`).join("\n"));
    manifest.sources.push({ id: `${input.root}:${input.file}`, root: input.root, file: input.file, required: input.required, referenceOnly: input.referenceOnly, authority: sourceAuthority(input), role: input.referenceOnly ? "localization-or-icon-reference" : input.root === "decoded" ? "exact-game-table" : "derived-research-input", bytes: raw.byteLength, sha256: hash(raw), semanticSha256, rowCount: Object.keys(fingerprints).length, collectionCounts, rowFingerprints: fingerprints });
  }
  manifest.sources.sort((a, b) => a.id.localeCompare(b.id));
  manifest.summary = { sourceCount: manifest.sources.length, rowCount: manifest.sources.reduce((sum, source) => sum + source.rowCount, 0), missingRequiredCount: manifest.missingRequiredInputs.length, missingOptionalCount: manifest.missingOptionalInputs.length };
  manifest.aggregateSha256 = hash(manifest.sources.map((source) => `${source.id}:${source.semanticSha256}`).join("\n"));
  return manifest;
}

function compareManifests(definition, baseline, candidate) {
  const before = flattenRows(baseline); const after = flattenRows(candidate);
  const beforeKeys = new Set(Object.keys(before)); const afterKeys = new Set(Object.keys(after));
  const addedRows = [...afterKeys].filter((key) => !beforeKeys.has(key)).sort();
  const removedRows = [...beforeKeys].filter((key) => !afterKeys.has(key)).sort();
  const changedRows = [...afterKeys].filter((key) => beforeKeys.has(key) && before[key] !== after[key]).sort();
  return { domain: definition.id, aggregateChanged: baseline.aggregateSha256 !== candidate.aggregateSha256, addedSources: sourceIds(candidate).filter((id) => !sourceIds(baseline).includes(id)), removedSources: sourceIds(baseline).filter((id) => !sourceIds(candidate).includes(id)), addedRows, removedRows, changedRows, changesByAuthority: summarizeByAuthority(baseline, candidate, addedRows, removedRows, changedRows), changeActions: definition.actions, proofSuites: definition.proofSuites };
}

function flattenRows(manifest) { const output = {}; for (const source of manifest.sources || []) for (const [key, value] of Object.entries(source.rowFingerprints || {})) output[`${source.id}#${key}`] = value; return output; }
function sourceIds(manifest) { return (manifest.sources || []).map((source) => source.id); }
function resolveCollection(value, collectionPath) { if (collectionPath === "$" ) return value; return collectionPath.split(".").reduce((current, key) => current?.[key], value); }
function* enumerateRows(value) { if (Array.isArray(value)) { for (let index = 0; index < value.length; index += 1) yield [rowIdentity(value[index], index), value[index]]; } else if (value && typeof value === "object") { for (const key of Object.keys(value).sort(naturalCompare)) yield [key, value[key]]; } else yield ["value", value]; }
function rowIdentity(row, index) { if (row && typeof row === "object") for (const key of ["uid", "Uid", "UID", "id", "Id", "ID", "buffId", "buff_id", "effectId", "effect_id", "skillId", "skill_id", "itemId", "item_id"]) if (row[key] !== undefined && row[key] !== null) return String(row[key]); return String(index); }
function sanitizeSemantic(value, parentKey = "") {
  if (Array.isArray(value)) return value.map((item) => sanitizeSemantic(item, parentKey));
  if (!value || typeof value !== "object") {
    if (typeof value === "string" && isProvenancePathKey(parentKey)) {
      return value.replace(/output-build-\d+/g, "output-build-{build}").replace(/steam-\d+/g, "steam-{build}");
    }
    return value;
  }
  const output = {};
  for (const key of Object.keys(value)) {
    if (isVolatileProvenanceKey(key) || key.toLowerCase() === "provenance") continue;
    output[key] = sanitizeSemantic(value[key], key);
  }
  return output;
}
function isVolatileProvenanceKey(key) {
  const normalized = key.replaceAll("_", "").replaceAll("-", "").toLowerCase();
  return normalized === "generatedat"
    || normalized === "generationtimestamp"
    || /source.*offsets?$/.test(normalized);
}
function isProvenancePathKey(key) {
  const normalized = key.replaceAll("_", "").replaceAll("-", "").toLowerCase();
  return normalized.endsWith("sourcepath") || normalized === "breakdownsource";
}
function canonical(value) { if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hash(value) { return createHash("sha256").update(value).digest("hex"); }
function decoded(file, required = true) { return { root: "decoded", file, required, referenceOnly: false, collections: ["$"] }; }
function extracted(file, collections, required = true, referenceOnly = false) { return { root: "extractor", file, required, referenceOnly, collections }; }
function sourceAuthority(input) { return input.referenceOnly ? "reference-only" : input.root === "decoded" ? "exact-game-table" : "derived-research"; }
function domain(id, inputs, actions, proofSuites) { return { id, inputs, actions, proofSuites: [...new Set([...proofSuites, "canonical-replay-conservation"])] }; }
function policy() { return { extractionRunsOutsideLiveParser: true, candidateDataNeverAutoPromoted: true, packetReplayRequiredForRuntimeRules: true, localizationBundlesRemainSeparate: true, iconReferencesDoNotBecomeMechanicsAuthority: true, allRowsRetained: true, unresolvedRowsHidden: false, rowFingerprintsAreResearchOnly: true }; }
function parseArgs(args) { const output = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase()); const next = args[index + 1]; if (!next || next.startsWith("--")) output[key] = true; else { output[key] = next; index += 1; } } return output; }
function required(options, key) { if (!options[key]) throw new Error(`Missing --${key.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`); return options[key]; }
function resolvePath(value) { return path.isAbsolute(value) ? value : path.resolve(repoRoot, value); }
function relativeRepo(value) { return path.relative(repoRoot, value).replaceAll("\\", "/"); }
function naturalCompare(a, b) { return a.localeCompare(b, undefined, { numeric: true }); }
function readJson(file) { return JSON.parse(readFileSync(file, "utf8")); }
function writeJson(file, value) { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`); }
function summarizeByAuthority(baseline, candidate, addedRows, removedRows, changedRows) {
  const authorityBySource = new Map();
  for (const source of [...(baseline.sources || []), ...(candidate.sources || [])]) authorityBySource.set(source.id, source.authority || (source.referenceOnly ? "reference-only" : source.root === "decoded" ? "exact-game-table" : "derived-research"));
  const summary = {};
  for (const [kind, rows] of [["added", addedRows], ["removed", removedRows], ["changed", changedRows]]) {
    for (const row of rows) {
      const sourceId = row.split("#", 1)[0];
      const authority = authorityBySource.get(sourceId) || "unknown";
      summary[authority] ||= { added: 0, removed: 0, changed: 0 };
      summary[authority][kind] += 1;
    }
  }
  return summary;
}
function selfTest() {
  const left = {
    sources: [{
      id: "decoded:A.json",
      authority: "exact-game-table",
      rowFingerprints: { "$:1": "a", "$:2": "b" },
    }],
    aggregateSha256: "left",
  };
  const right = {
    sources: [{
      id: "decoded:A.json",
      authority: "exact-game-table",
      rowFingerprints: { "$:1": "a", "$:2": "c", "$:3": "d" },
    }],
    aggregateSha256: "right",
  };
  const result = compareManifests(domains[0], left, right);
  if (result.addedRows.length !== 1 || result.changedRows.length !== 1 || result.removedRows.length !== 0) {
    throw new Error("row-diff self-test failed");
  }
  if (result.changesByAuthority["exact-game-table"]?.added !== 1
    || result.changesByAuthority["exact-game-table"]?.changed !== 1) {
    throw new Error("authority summary self-test failed");
  }

  const priorImagine = {
    uid: 71001,
    tier: 4,
    coefficient: 520,
    generatedAt: "2026-08-01T00:00:00Z",
    sourceOffset: "0x1000",
    provenance: { build: "24609362", sourcePath: "output-build-24609362/SkillAoyiStarTable.json" },
  };
  const sameImagineFromNewScan = {
    uid: 71001,
    tier: 4,
    coefficient: 520,
    generatedAt: "2026-08-15T00:00:00Z",
    sourceOffset: "0x9000",
    provenance: { build: "24687926", sourcePath: "output-build-24687926/SkillAoyiStarTable.json" },
  };
  const rescaledImagine = { ...sameImagineFromNewScan, coefficient: 540 };
  const priorFingerprint = hash(canonical(sanitizeSemantic(priorImagine)));
  const sameFingerprint = hash(canonical(sanitizeSemantic(sameImagineFromNewScan)));
  const rescaledFingerprint = hash(canonical(sanitizeSemantic(rescaledImagine)));
  if (priorFingerprint !== sameFingerprint) throw new Error("volatile scan metadata changed the semantic fingerprint");
  if (priorFingerprint === rescaledFingerprint) throw new Error("Imagine scaling change was not detected");

  console.log("seasonal domain scanner self-test passed");
}
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-seasonal-domain-scan.mjs scan --build <build> [--extractor-root <path>] [--decoded-root <path>]\n  node tools/bpsr-seasonal-domain-scan.mjs diff --baseline-build <build> --candidate-build <build>\n  node tools/bpsr-seasonal-domain-scan.mjs self-test"); process.exit(exitCode); }
