#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const paths = Object.fromEntries([
  "inventory", "fightAttributeProof", "historicalFalconryProof", "snapshotProof",
  "castSnapshotProof", "skills", "buffs", "skillTable", "effectSources", "output",
].map((key) => [key, resolvePath(options[key])]));

const inventory = readJson(paths.inventory, "current-build Mastery consumer inventory");
const fightAttributeProof = readJson(paths.fightAttributeProof, "fight-attribute evaluator proof");
const historicalFalconryProof = readJson(paths.historicalFalconryProof, "historical Falconry transition proof");
const snapshotProof = readJson(paths.snapshotProof, "Mastery/property snapshot proof");
const castSnapshotProof = readJson(paths.castSnapshotProof, "cast/damage snapshot proof");
const skills = readJson(paths.skills, "current-build skill identity catalog");
const buffs = readJson(paths.buffs, "current-build buff identity catalog");
const skillTable = readJson(paths.skillTable, "current-build SkillTable");
const effectSources = readJson(paths.effectSources, "current-build effect-source graph");
const expectedBuild = String(options.gameBuild);
const expectedPacketBuild = String(options.packetBuild);

validateInputs();
const components = buildComponents();
const inactiveDescriptors = (inventory.unpaired_descriptors || []).map((row) => ({
  profession_id: Number(row.profession_id),
  profession_name: row.profession_name,
  descriptor_index: Number(row.descriptor_index),
  description: row.description,
  disposition: "retained-inactive-or-unreleased-no-active-talent-stage",
  runtime_authority: false,
}));

const result = {
  schema_version: 1,
  generated_by: "tools/mastery-property-offline-exhaustion-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: expectedBuild,
  packet_build: expectedPacketBuild,
  proof_state: "offline-mastery-client-and-archive-exhausted-final-validation-required",
  policy: {
    exact_build_required: true,
    unresolved_evidence_is_hidden: false,
    localized_descriptions_are_runtime_formula_authority: false,
    character_sheet_mastery_curve_is_combat_stage_authority: false,
    historical_transition_is_current_build_authority: false,
    isolated_transition_is_absolute_property_formula_authority: false,
    latest_serialized_attribute_is_action_snapshot_authority: false,
    candidate_components_are_executable: false,
    no_component_is_omitted_because_it_is_non_damage: true,
    matching_build_packet_validation_is_required: true,
    future_active_spec_or_changed_description_reopens_gate: true,
  },
  inputs: Object.fromEntries(Object.entries(paths)
    .filter(([key]) => key !== "output")
    .map(([key, value]) => [key, relative(value)])),
  summary: {
    offline_exhausted_model_ids: ["mastery-property-transform"],
    active_classes: new Set(components.map((row) => row.class_id)).size,
    active_specs: new Set(components.map((row) => row.specialization_id)).size,
    candidate_components: components.length,
    final_validation_obligations: components.length,
    damage_or_action_components: components.filter((row) => row.metric_domain !== "non-damage").length,
    non_damage_components_retained: components.filter((row) => row.metric_domain === "non-damage").length,
    inactive_or_unreleased_descriptors_retained: inactiveDescriptors.length,
    historical_falconry_delayed_exact_matches: Number(historicalFalconryProof.proof.delayed_exact_matches),
    historical_falconry_delayed_mismatches: Number(historicalFalconryProof.proof.delayed_mismatches),
    delayed_snapshot_gap_damage_events: Number(snapshotProof.counters.gap_examples_present_in_cohort),
    nearby_same_calculation_identity_controls: Number(snapshotProof.counters.stable_controls_with_same_calculation_identity),
    strict_state_control_pairs: Number(snapshotProof.counters.strict_state_control_pairs),
    decoded_cast_events: Number(castSnapshotProof.coverage.cast_events),
    damage_events_with_ability_and_hit: Number(castSnapshotProof.coverage.damage_events_with_ability_and_hit),
    promoted_runtime_components: 0,
  },
  mastery_display_transform: {
    input_attribute_id: 11140,
    derived_attribute_id: 11940,
    parameters: [50000, 1, 1, 0, 0, 0, 0],
    exact_character_sheet_expression: "100 * raw / (raw + 50000)",
    underlying_value_rounding: "none in proven client UI evaluator",
    combat_authority: false,
  },
  components,
  inactive_or_unreleased_descriptors: inactiveDescriptors,
  final_validation: components.map((row) => ({
    obligation_id: `mastery:${row.specialization_id}:${row.component_index}`,
    class_id: row.class_id,
    specialization_id: row.specialization_id,
    component_kind: row.component_kind,
    requirement: "matching-build recipient specialization, Mastery/property lifecycle, exact consumer identity, server snapshot boundary, integer ordering/rounding, provider-removed counterfactual, and event/aggregate conservation",
  })),
};

writeFileSync(paths.output, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function validateInputs() {
  if (String(inventory.game_build) !== expectedBuild
    || String(fightAttributeProof.game_build) !== expectedBuild) {
    throw new Error("Mastery input build differs from requested current build");
  }
  if (String(historicalFalconryProof.client_build) !== expectedPacketBuild) {
    throw new Error("historical Falconry proof differs from requested packet build");
  }
  if (inventory.promotion_state !== "audit-only-no-runtime-authority"
    || inventory.policy?.localized_descriptions_are_runtime_authority !== false
    || inventory.policy?.unresolved_descriptors_hidden !== false
    || inventory.policy?.matching_build_packet_replay_required !== true) {
    throw new Error("Mastery inventory violates fail-closed policy");
  }
  if (Number(inventory.summary?.profession_rows) !== 13
    || Number(inventory.summary?.paired_consumers) !== 18
    || Number(inventory.summary?.unpaired_descriptors) !== 8
    || Number(inventory.summary?.unpaired_talent_stages) !== 0) {
    throw new Error("current-build Mastery inventory coverage changed");
  }
  const rowThree = (fightAttributeProof.rows || []).find((row) => Number(row.season_id) === 3);
  const mastery = rowThree?.fields?.MasteryToMasteryPct;
  if (fightAttributeProof.proof_state !== "exact-current-build-client-ui-evaluator"
    || fightAttributeProof.policy?.combat_damage_stage_authority !== false
    || mastery?.state !== "exact-current-build-parameter-array"
    || JSON.stringify(mastery.parameters) !== JSON.stringify([50000, 1, 1, 0, 0, 0, 0])) {
    throw new Error("current-build Mastery display transform changed");
  }
  if (historicalFalconryProof.accounting_policy?.absolute_light_formula_proven !== false
    || historicalFalconryProof.accounting_policy?.hit_time_latest_attribute_state_is_formula_authority !== false
    || Number(historicalFalconryProof.proof?.delayed_exact_matches) !== 143
    || Number(historicalFalconryProof.proof?.delayed_mismatches) !== 0
    || Number(historicalFalconryProof.wire_boundary_proof?.gap_damage_events) !== 397
    || Number(historicalFalconryProof.wire_boundary_proof?.gap_light_damage_events) !== 374) {
    throw new Error("historical Falconry transition proof changed");
  }
  if (snapshotProof.policy?.runtime_authority !== false
    || snapshotProof.policy?.unresolved_evidence_is_hidden !== false
    || Number(snapshotProof.counters?.gap_examples_present_in_cohort) !== 374
    || Number(snapshotProof.counters?.stable_controls_with_same_calculation_identity) !== 264
    || Number(snapshotProof.counters?.strict_state_control_pairs) !== 0) {
    throw new Error("Mastery/property snapshot exhaustion proof changed");
  }
  if (castSnapshotProof.policy?.runtime_formula_authority !== false
    || castSnapshotProof.policy?.unresolved_evidence_is_hidden !== false
    || Number(castSnapshotProof.coverage?.cast_events) !== 0
    || Number(castSnapshotProof.coverage?.damage_events_with_ability_and_hit) !== 882) {
    throw new Error("cast/damage snapshot proof changed");
  }
}

function buildComponents() {
  const definitions = new Map([
    [101, [["skill-filtered-damage", 3.4, "damage", "skills that use Thunder Sigils"]]],
    [102, [["skill-filtered-damage", 1.2, "damage", "Thundercut, Storm Scythe, and Divine Sickle"]]],
    [104, [["element-bonus", 0.65, "damage", "ice"]]],
    [105, [
      ["resource-gain-efficiency", 1, "action-opportunity", "ice energy"],
      ["element-bonus", 0.2, "damage", "ice"],
      ["mastery-enhancement-conversion-ratio", 3, "derived-state", "ice bonus conversion ratio"],
    ]],
    [128, [
      ["skill-filtered-cooldown-boost", 3.2, "action-opportunity", "Blazing Assault, Rage Cleave, and Axe Wind"],
      ["attack-percent", 0.2, "damage", "attack"],
    ]],
    [129, [["element-bonus", 0.72, "damage", "fire"]]],
    [107, [
      ["element-bonus", 0.35, "damage", "wind"],
      ["mastery-enhancement-conversion-ratio", 3, "derived-state", "wind bonus conversion ratio"],
    ]],
    [108, [["element-bonus", 0.65, "damage", "wind"]]],
    [110, [
      ["element-bonus", 0.75, "damage", "forest"],
      ["shield-strength", 0.3, "non-damage", "shield strength"],
    ]],
    [111, [["healing", 1, "non-damage", "healing"]]],
    [113, [["shield-strength", 2.5, "non-damage", "shield strength"]]],
    [114, [
      ["block-damage-reduction", 0.2, "non-damage", "block damage reduction"],
      ["lucky-block-damage-reduction", 0.2, "non-damage", "lucky block damage reduction"],
    ]],
    [116, [["companion-damage", 2.75, "damage", "companion damage"]]],
    [117, [["element-bonus", 0.6, "damage", "light"]]],
    [122, [
      ["named-shield-gain", 2.5, "non-damage", "Radiant Shield"],
      ["all-element-resistance", 0.2, "non-damage", "all element resistance"],
    ]],
    [123, [
      ["named-barrier-hp", 3, "non-damage", "Lightforged Barrier"],
      ["all-element-resistance", 0.2, "non-damage", "all element resistance"],
    ]],
    [119, [["named-conversion-bonus", 1, "derived-state", "Peaceful Tune"]]],
    [120, [["named-decay-speed-reduction", 0.35, "non-damage", "Healing Melody"]]],
  ]);
  const consumers = inventory.consumers || [];
  if (consumers.length !== definitions.size) throw new Error("Mastery active-spec definition count changed");
  const rows = [];
  for (const consumer of consumers) {
    const specializationId = Number(consumer.talent_stage_id);
    const entries = definitions.get(specializationId);
    if (!entries) throw new Error(`no exact component catalog for Mastery specialization ${specializationId}`);
    const numeric = (consumer.numeric_literals || []).map(Number).filter(Number.isFinite);
    for (const [index, [kind, coefficient, domain, scope]] of entries.entries()) {
      if (!numeric.some((value) => value === coefficient)) {
        throw new Error(`Mastery component ${specializationId}:${index} coefficient ${coefficient} is absent from current description`);
      }
      rows.push({
        class_id: Number(consumer.profession_id),
        class_name: consumer.profession_name,
        specialization_id: specializationId,
        specialization_name: String(consumer.talent_stage_name).replace(/ Spec$/, ""),
        component_index: index,
        component_kind: kind,
        displayed_percent_per_one_percent_mastery: coefficient,
        scope,
        metric_domain: domain,
        validation_route: masteryValidationRoute(kind),
        validation_property_ids: masteryValidationPropertyIds(kind, scope),
        required_event_kinds: masteryRequiredEventKinds(kind),
        selectors: masterySelectors(specializationId, index, kind, scope),
        description: consumer.description,
        evidence_state: specializationId === 117 && kind === "element-bonus"
          ? "historical-isolated-transition-exact-current-build-validation-required"
          : "current-build-client-description-candidate-final-validation-required",
        runtime_authority: false,
      });
    }
  }
  if (rows.length !== 26) throw new Error(`expected 26 Mastery components, found ${rows.length}`);
  return rows;
}

function masteryValidationRoute(kind) {
  if (kind === "skill-filtered-damage") return "outgoing-selected-ability-damage";
  if (kind === "skill-filtered-cooldown-boost") return "selected-ability-cooldown-transition";
  if (kind === "resource-gain-efficiency") return "owned-resource-transition";
  if (kind === "companion-damage") return "owned-companion-outgoing-damage";
  if (kind === "healing") return "outgoing-healing";
  if (kind === "named-shield-gain") return "named-shield-state";
  if (kind === "named-barrier-hp") return "named-status-lifecycle";
  if (kind === "shield-strength") {
    return "outgoing-shield-or-barrier-state";
  }
  if (["block-damage-reduction", "lucky-block-damage-reduction", "all-element-resistance"].includes(kind)) {
    return "incoming-damage-mitigation";
  }
  if (kind === "named-decay-speed-reduction") return "named-resource-decay-lifecycle";
  if (kind === "named-conversion-bonus") return "named-skill-output";
  return "outgoing-damage";
}

function masteryRequiredEventKinds(kind) {
  const common = ["actor", "entity_attributes"];
  if (kind === "skill-filtered-cooldown-boost") return [...common, "cooldown"];
  if (kind === "resource-gain-efficiency") return [...common, "resource"];
  if (kind === "healing") return [...common, "healing"];
  if (kind === "named-shield-gain") return [...common, "status", "shield_state"];
  if (kind === "named-barrier-hp") return [...common, "status"];
  if (kind === "shield-strength") {
    return [...common, "shield_state"];
  }
  if (kind === "named-decay-speed-reduction") return [...common, "status", "resource"];
  return [...common, "damage"];
}

function masteryValidationPropertyIds(kind, scope) {
  if (kind !== "element-bonus" && kind !== "all-element-resistance") return [];
  if (kind === "all-element-resistance") return [1, 2, 3, 4, 5, 6, 7, 8];
  const properties = {
    fire: 1,
    ice: 2,
    forest: 4,
    wind: 5,
    light: 7,
  };
  const property = properties[String(scope || "").toLowerCase()];
  if (!Number.isInteger(property)) {
    throw new Error(`missing packet property mapping for Mastery element scope ${scope}`);
  }
  return [property];
}

function masterySelectors(specializationId, componentIndex, kind, scope) {
  let names = [];
  if (specializationId === 101 && componentIndex === 0) {
    names = thunderSigilConsumerNames();
  } else if (specializationId === 102 && componentIndex === 0) {
    names = ["Thundercut", "Storm Scythe", "Divine Sickle"];
  } else if (specializationId === 128 && componentIndex === 0) {
    names = ["Blazing Assault", "Rage Cleave", "Axe Wind"];
  } else if (["named-shield-gain", "named-barrier-hp", "named-conversion-bonus", "named-decay-speed-reduction"].includes(kind)) {
    names = [scope];
  }
  const selectors = skillFamilySelectors(names);
  if (["named-shield-gain", "named-barrier-hp", "named-decay-speed-reduction"].includes(kind)) {
    // These routes are defined by an exact current-build status identity. Do not
    // retain broad same-name skill or damage families: those can include NPC or
    // output rows that share a localized name and would create false direct hits.
    selectors.skill_ids = [];
    selectors.damage_ids = [];
    selectors.recount_ids = [];
    selectors.effect_ids = masteryRuntimeBuffIds(specializationId, kind, names);
  }
  return selectors;
}

function thunderSigilConsumerNames() {
  return [...new Set(Object.values(skills)
    .filter((row) => (row.Kinds || []).includes("skill-table"))
    .filter((row) => /consum(?:e|es|ing) (?:all |\d+ )?Thunder Sigils?/i.test(String(row.Notes?.en || "")))
    .map((row) => String(row.Name || "").trim())
    .filter(Boolean))].sort();
}

function skillFamilySelectors(names) {
  const wanted = new Set(names.map((name) => name.toLowerCase()));
  const rows = Object.values(skills).filter((row) => wanted.has(String(row.Name || "").toLowerCase()));
  const skillIds = [];
  const damageIds = [];
  const recountIds = [];
  for (const row of rows) {
    const kinds = new Set(row.Kinds || []);
    if (kinds.has("skill-table") || kinds.has("skill-effect") || kinds.has("skill-fight")) {
      skillIds.push(Number(row.Id));
      skillIds.push(...(row.SkillEffectIds || []).map(Number));
    }
    if (kinds.has("damage-attr")) damageIds.push(Number(row.Id));
    recountIds.push(...(row.RecountIds || []).map(Number));
  }
  return {
    source_names: [...wanted].sort(),
    skill_ids: uniqueSafeIntegers(skillIds),
    damage_ids: uniqueSafeIntegers(damageIds),
    recount_ids: uniqueSafeIntegers(recountIds),
  };
}

function directBuffIds(names) {
  const wanted = new Set(names.map((name) => name.toLowerCase()));
  const ids = [];
  for (const [uid, value] of Object.entries(buffs.entriesByUid || {})) {
    const rows = Array.isArray(value) ? value : [value];
    if (!rows.some((row) => wanted.has(String(row?.names?.en || row?.name || "").trim().toLowerCase()))) {
      continue;
    }
    ids.push(Number(uid));
  }
  return uniqueSafeIntegers(ids);
}

function masteryRuntimeBuffIds(specializationId, kind, names) {
  if (specializationId === 120 && kind === "named-decay-speed-reduction") {
    const healingMelody = Object.values(skillTable)
      .find((row) => String(row?.Name || "").trim().toLowerCase() === "healing melody");
    if (!healingMelody) throw new Error("current-build Healing Melody SkillTable row is missing");
    const ids = (healingMelody.SwitchSkillInfo || [])
      .flatMap((row) => Array.isArray(row) ? row.slice(1) : [])
      .map(Number)
      .filter((id) => Number.isSafeInteger(id) && buffs.entriesByUid?.[String(id)]);
    if (ids.length !== 1) {
      throw new Error(`expected one exact Healing Melody active-state buff, found ${ids.join(",") || "none"}`);
    }
    return uniqueSafeIntegers(ids);
  }
  if (specializationId === 122 && kind === "named-shield-gain") {
    const ids = Object.values(effectSources.effectSourcesById || {})
      .filter((row) => row?.sourceType === "talent")
      .filter((row) => String(row?.sourceNames?.en || row?.sourceName || "").trim().toLowerCase() === "radiant shield")
      .filter((row) => (row.targets || []).some((target) => target.producedOutputKind === "shield"))
      .flatMap((row) => row.buffIds || [])
      .map(Number);
    if (ids.length !== 1) {
      throw new Error(`expected one exact talent-linked Radiant Shield buff, found ${ids.join(",") || "none"}`);
    }
    return uniqueSafeIntegers(ids);
  }
  const ids = directBuffIds(names);
  if (ids.length === 0) {
    throw new Error(`no exact current-build buff identity for ${names.join(", ")}`);
  }
  return ids;
}

function uniqueSafeIntegers(values) {
  return [...new Set(values.filter(Number.isSafeInteger))].sort((left, right) => left - right);
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
    "gameBuild", "packetBuild", "inventory", "fightAttributeProof",
    "historicalFalconryProof", "snapshotProof", "castSnapshotProof", "skills", "output",
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
