#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const inputs = {
  factorClosure: resolvePath(options.factorClosure),
  remainingTrace: resolvePath(options.remainingTrace),
  globalGate: resolvePath(options.globalGate),
  targetMitigation: resolvePath(options.targetMitigation),
  mastery: resolvePath(options.mastery),
  damageStage: resolvePath(options.damageStage),
};
const outputPath = resolvePath(options.output);
const runtimeOutputPath = resolvePath(options.runtimeOutput);
const validationReportSchema = Number(options.reportSchema);
if (!Number.isSafeInteger(validationReportSchema) || validationReportSchema <= 0) {
  throw new Error("--reportSchema must be a positive integer");
}

const factorClosure = readJson(inputs.factorClosure, "Psychoscope factor closure");
const remainingTrace = readJson(inputs.remainingTrace, "remaining rDPS trace");
const globalGate = readJson(inputs.globalGate, "global offline gate");
const targetMitigation = readJson(inputs.targetMitigation, "target mitigation proof");
const mastery = readJson(inputs.mastery, "Mastery proof");
const damageStage = readJson(inputs.damageStage, "damage-stage catalog");

const EFFECT_KEYS = new Set(["effect_id", "effect_ids", "buff_id", "buff_ids", "source_buff_id", "source_buff_ids"]);
const SKILL_KEYS = new Set(["skill_id", "skill_ids", "ability_id", "ability_ids", "base_skill_id"]);
const DAMAGE_KEYS = new Set(["damage_id", "damage_ids", "produced_damage_id", "produced_damage_ids", "affected_damage_id"]);
const RECOUNT_KEYS = new Set(["recount_id", "recount_ids", "parent_recount_id", "affected_recount_id"]);
const ATTRIBUTE_KEYS = new Set(["attribute_id", "attribute_ids", "attr_id", "attr_ids", "property_id"]);
const CLASS_KEYS = new Set(["class_id", "class_ids", "class_gate_ids"]);
const SPECIALIZATION_KEYS = new Set(["specialization_id", "specialization_ids", "spec_id", "spec_ids"]);
const ITEM_KEYS = new Set(["item_id", "item_ids", "grade_item_ids"]);

const gameBuild = String(globalGate.game_build || "");
assertBuild(factorClosure, gameBuild, "Psychoscope factor closure");
assertBuild(remainingTrace, gameBuild, "remaining rDPS trace", "static_game_build");
assertBuild(targetMitigation, gameBuild, "target mitigation proof");
assertBuild(mastery, gameBuild, "Mastery proof");
assertBuild(damageStage, gameBuild, "damage-stage catalog");
if (!globalGate.summary?.capture_ready || globalGate.summary?.offline_obligations_remaining !== 0) {
  throw new Error("global offline gate has not authorized matching-build validation");
}

const leaves = [
  ...factorLeaves(),
  ...runtimeGateLeaves(),
  ...packetRouteLeaves(),
  ...targetMitigationLeaves(),
  ...masteryLeaves(),
];
validateLeaves(leaves);

const expected = Number(
  globalGate.summary?.final_validation_obligations_held_until_offline_zero,
);
if (!Number.isSafeInteger(expected) || expected <= 0 || leaves.length !== expected) {
  throw new Error(`normalized ${leaves.length} obligations, expected ${expected}`);
}

const manifest = {
  schema_version: 2,
  generated_by: "tools/rdps-matching-build-validation-manifest.mjs",
  game: "Blue Protocol: Star Resonance",
  game_build: gameBuild,
  validation_report_schema: validationReportSchema,
  policy: {
    offline_gate_satisfied: true,
    canonical_events_retained: true,
    unresolved_evidence_hidden: false,
    guessed_relationships_allowed: false,
    one_streaming_pass: true,
    rule_scan_per_event: false,
    build_mismatch_behavior: "warn-and-provisionally-evaluate-unchanged-routes",
    unknown_or_changed_routes: "retain-without-invented-attribution",
  },
  inputs: Object.fromEntries(
    Object.entries(inputs).map(([key, filePath]) => [key, artifactReference(filePath)]),
  ),
  summary: {
    total_obligations: leaves.length,
    pending_matching_build_obligations: leaves.length,
    by_domain: countBy(leaves, (leaf) => leaf.domain),
    by_required_event_kind: countMany(leaves, (leaf) => leaf.required_event_kinds),
    indexed_effect_ids: countIndexValues(leaves, "effect_ids"),
    indexed_skill_ids: countIndexValues(leaves, "skill_ids"),
    indexed_damage_ids: countIndexValues(leaves, "damage_ids"),
    indexed_recount_ids: countIndexValues(leaves, "recount_ids"),
    indexed_attribute_ids: countIndexValues(leaves, "attribute_ids"),
    indexed_formula_input_attribute_ids: new Set(
      leaves.flatMap((leaf) =>
        (leaf.formula_inputs || []).flatMap((input) => [
          ...(input.candidate_attribute_ids || []),
          ...(input.class_attribute_routes || []).flatMap((route) => route.candidate_attribute_ids || []),
        ]),
      ),
    ).size,
    indexed_source_config_ids: countIndexValues(leaves, "source_config_ids"),
    indexed_class_ids: countIndexValues(leaves, "class_ids"),
    indexed_specialization_ids: countIndexValues(leaves, "specialization_ids"),
    indexed_item_ids: countIndexValues(leaves, "item_ids"),
    indexed_equipment_suit_pairs: new Set(
      leaves.flatMap((leaf) =>
        leaf.selectors.equipment_suit_entries.map(
          (entry) => `${entry.map_key}:${entry.attribute_key}`,
        ),
      ),
    ).size,
  },
  indexes: buildIndexes(leaves),
  damage_packet_selectors: buildDamagePacketSelectors(leaves),
  obligations: leaves,
};

function buildDamagePacketSelectors(leaves) {
  const wanted = new Set(leaves.flatMap((leaf) => leaf.selectors.damage_ids));
  const recountBacked = new Set(
    leaves.flatMap((leaf) => leaf.selectors.recount_ids),
  );
  const selectors = [];
  visitDamageStage(damageStage, (row) => {
    const damageId = Number(row.damage_attr_id);
    if (!wanted.has(damageId)) return;
    const abilityId = Number(row.ability_id ?? row.linked_ability_id);
    const hitEventId = Number(row.hit_event_id ?? row.hit_event_suffix_candidate);
    if (!Number.isSafeInteger(abilityId) || !Number.isSafeInteger(hitEventId)) return;
    selectors.push({ damage_id: damageId, ability_id: abilityId, hit_event_id: hitEventId });
  });
  const unique = [...new Map(selectors.map((row) => [
    `${row.damage_id}:${row.ability_id}:${row.hit_event_id}`,
    row,
  ])).values()].sort((left, right) =>
    left.damage_id - right.damage_id
      || left.ability_id - right.ability_id
      || left.hit_event_id - right.hit_event_id
  );
  const resolved = new Set(unique.map((row) => row.damage_id));
  // Some client identity rows intentionally expose their stable recount parent
  // through both the damage and recount catalogs. Those compact parent IDs do
  // not have an ability/hit pair in DamageAttr, but the canonical damage event
  // still matches them exactly through recount_id.
  const missing = [...wanted].filter(
    (damageId) => !resolved.has(damageId) && !recountBacked.has(damageId),
  );
  if (missing.length > 0) {
    throw new Error(`damage-stage catalog did not resolve packet selectors for: ${missing.join(", ")}`);
  }
  return unique;
}

function visitDamageStage(value, observer) {
  if (value === null || value === undefined || typeof value !== "object") return;
  if (!Array.isArray(value) && Number.isSafeInteger(Number(value.damage_attr_id))) observer(value);
  for (const child of Object.values(value)) visitDamageStage(child, observer);
}

writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
const runtimeWatch = {
  schema_version: manifest.schema_version,
  game_build: manifest.game_build,
  validation_report_schema: manifest.validation_report_schema,
  authority: "candidate-evidence-watch-only",
  policy: {
    proof_promotion_allowed: false,
    build_mismatch_behavior: manifest.policy.build_mismatch_behavior,
    unknown_or_changed_routes: manifest.policy.unknown_or_changed_routes,
  },
  damage_packet_selectors: manifest.damage_packet_selectors,
  obligations: manifest.obligations.map((row) => ({
    obligation_id: row.obligation_id,
    domain: row.domain,
    subject_kind: row.subject_kind,
    subject_id: row.subject_id,
    subject_name: row.subject_name,
    requirements: row.requirements,
    required_event_kinds: row.required_event_kinds,
    selectors: row.selectors,
    formula_inputs: row.formula_inputs,
    evidence: {
      validation_route: row.evidence?.validation_route,
      component_kind: row.evidence?.component_kind,
      property_ids: row.evidence?.property_ids || [],
    },
  })),
};
writeFileSync(runtimeOutputPath, `${JSON.stringify(runtimeWatch)}\n`, "utf8");
console.log(`Wrote ${relativeRepoPath(outputPath)}`);
console.log(`Wrote ${relativeRepoPath(runtimeOutputPath)}`);
console.log(JSON.stringify(manifest.summary, null, 2));

function factorLeaves() {
  const rows = (factorClosure.families || []).filter(
    (family) => family.current_runtime_eligible
      && (family.final_validation_obligations || []).length > 0,
  );
  assertCount(
    "Psychoscope factor-family validation",
    rows.length,
    factorClosure.summary?.total_final_validation_obligations,
  );
  return rows.map((family) => leaf({
    obligation_id: `factor-family:${family.family_id}`,
    domain: "psychoscope-factor",
    subject_kind: "factor-family",
    subject_id: String(family.family_id),
    subject_name: family.family_name || `Factor family ${family.family_id}`,
    requirements: family.final_validation_obligations,
    selectors: {
      source_rule_ids: [],
      effect_ids: family.source_buff_ids,
      skill_ids: family.exact_skill_ids,
      damage_ids: [
        ...(family.direct_damage_ids || []),
        ...(family.generated_damage_families || []),
      ],
      recount_ids: family.exact_recount_ids,
      attribute_ids: collectIds(family.state_routes, ATTRIBUTE_KEYS),
      class_ids: family.class_gate_ids,
      specialization_ids: [],
      item_ids: family.grade_item_ids,
      equipment_suit_entries: [],
      source_config_ids: [],
    },
    evidence: {
      runtime_role: family.runtime_role,
      slot_category: family.slot_category,
      mechanic_classes: family.mechanic_classes || [],
      runtime_selectors: family.runtime_selectors || [],
      offline_route_state: family.offline_route_state,
    },
    required_event_kinds: factorRequiredEventKinds(family),
  }));
}

function factorRequiredEventKinds(family) {
  const kinds = new Set(["profile_selection"]);
  if ((family.source_buff_ids || []).length > 0) kinds.add("status");
  if ((family.energy_behaviors || []).some((behavior) =>
    behavior === "generate" || behavior === "consume-at-threshold")) kinds.add("resource");
  if ((family.mechanic_classes || []).includes("cooldown-manipulation")) kinds.add("cooldown");
  if ((family.mechanic_classes || []).some((mechanic) => [
    "triggered-damage-output", "summoned-output", "healing-output", "shield-output",
  ].includes(mechanic))) kinds.add("damage");
  return [...kinds].sort();
}

function runtimeGateLeaves() {
  const rows = [];
  for (const candidate of remainingTrace.candidates || []) {
    const gates = candidate.proof_gates?.gates || {};
    for (const [gateName, gate] of Object.entries(gates)) {
      if (String(gate.status || "").startsWith("proven")) continue;
      const selectors = selectorsFrom(candidate);
      rows.push(leaf({
        obligation_id: `runtime-gate:${candidate.source_rule_id}:${gateName}`,
        domain: "offensive-runtime-gate",
        subject_kind: "source-rule",
        subject_id: String(candidate.source_rule_id),
        subject_name: candidate.source_name || candidate.source_id || candidate.source_rule_id,
        requirements: [gateName],
        selectors,
        evidence: {
          gate_status: gate.status,
          gate_detail: gate.detail,
          retained_evidence: gate.evidence || [],
          formula_routes: candidate.formula_routes || [],
          final_validation_obligations: candidate.final_validation_obligations || [],
        },
      }));
    }
  }
  assertCount(
    "offensive runtime gates",
    rows.length,
    remainingTrace.summary?.failed_proof_gates,
  );
  return rows;
}

function packetRouteLeaves() {
  const rows = globalGate.packet_bound_produced_damage_routes || [];
  assertCount(
    "packet-bound produced-damage routes",
    rows.length,
    globalGate.summary?.packet_bound_produced_damage_routes?.matching_build_packet_bindings_required,
  );
  return rows.map((route) => leaf({
    obligation_id: `packet-output-route:${route.source_rule_id}`,
    domain: "packet-output-route",
    subject_kind: "source-rule",
    subject_id: String(route.source_rule_id),
    subject_name: route.source_name || route.source_id || route.source_rule_id,
    requirements: route.required_runtime_evidence || [],
    selectors: {
      source_rule_ids: [route.source_rule_id],
      effect_ids: route.effect_ids,
      skill_ids: collectIds(route, SKILL_KEYS),
      damage_ids: collectIds(route, DAMAGE_KEYS),
      recount_ids: collectIds(route, RECOUNT_KEYS),
      // Formula inputs are state retained before a trigger, not direct route
      // selectors. Keeping them separate prevents an ordinary attribute update
      // from falsely activating a packet-output obligation.
      attribute_ids: [],
      class_ids: [],
      specialization_ids: [],
      item_ids: [],
      equipment_suit_entries: [],
      source_config_ids: [],
    },
    evidence: {
      offline_route_state: route.offline_route_state,
      static_reference_evidence: route.static_reference_evidence,
    },
    formula_inputs: route.formula_inputs || [],
    required_event_kinds: ["damage", "formula_inputs", "status"],
  }));
}

function targetMitigationLeaves() {
  const rows = targetMitigation.final_validation || [];
  assertCount(
    "target mitigation counterfactuals",
    rows.length,
    targetMitigation.summary?.final_validation_obligations,
  );
  return rows.map((row) => leaf({
    obligation_id: `target-mitigation:${row.model_id}`,
    domain: "target-mitigation",
    subject_kind: "formula-model",
    subject_id: row.model_id,
    subject_name: row.model_id,
    requirements: [row.requirement],
    selectors: {
      source_rule_ids: [], effect_ids: [], skill_ids: [], damage_ids: [], recount_ids: [],
      attribute_ids: collectIds(targetMitigation, ATTRIBUTE_KEYS),
      class_ids: [], specialization_ids: [], item_ids: [],
      equipment_suit_entries: [],
      source_config_ids: [],
    },
    evidence: {
      proof_state: targetMitigation.proof_state,
      current_build_client_candidates: targetMitigation.current_build_client_candidates,
    },
    // Target status equality is a counterfactual-pair constraint evaluated
    // from the retained timeline, not a selectable event identity. Requiring a
    // generic status event here would be impossible to satisfy deterministically
    // because these model obligations intentionally have no effect selector.
    required_event_kinds: ["damage", "entity_attributes", "temporary_attributes"],
  }));
}

function masteryLeaves() {
  const componentsById = new Map(
    (mastery.components || []).map((component) => [
      component.obligation_id || masteryObligationId(component),
      component,
    ]),
  );
  const rows = mastery.final_validation || [];
  assertCount(
    "Mastery-property components",
    rows.length,
    mastery.summary?.final_validation_obligations,
  );
  return rows.map((row) => {
    const component = componentsById.get(row.obligation_id);
    if (!component) {
      throw new Error(
        `Mastery validation obligation ${row.obligation_id} has no matching component`,
      );
    }
    return leaf({
      obligation_id: row.obligation_id,
      domain: "mastery-property",
      subject_kind: "specialization-component",
      subject_id: row.obligation_id,
      subject_name: component.label || row.component_kind || row.obligation_id,
      requirements: [row.requirement],
      required_event_kinds: component.required_event_kinds,
      selectors: {
        source_rule_ids: [],
        effect_ids: collectIds(component, EFFECT_KEYS),
        skill_ids: collectIds(component, SKILL_KEYS),
        damage_ids: collectIds(component, DAMAGE_KEYS),
        recount_ids: collectIds(component, RECOUNT_KEYS),
        attribute_ids: uniqueNumbers([
          ...collectIds(component, ATTRIBUTE_KEYS),
          ...collectIds(mastery.mastery_display_transform, ATTRIBUTE_KEYS),
        ]),
        class_ids: [row.class_id],
        specialization_ids: [row.specialization_id],
        item_ids: [],
        equipment_suit_entries: [],
        source_config_ids: [],
      },
      evidence: {
        component_kind: row.component_kind,
        validation_route: component.validation_route,
        property_ids: component.validation_property_ids || [],
        component,
      },
    });
  });
}

function masteryObligationId(component) {
  return `mastery:${component.specialization_id}:${component.component_index ?? 0}`;
}

function selectorsFrom(candidate) {
  return {
    source_rule_ids: [candidate.source_rule_id],
    effect_ids: uniqueNumbers([
      ...(candidate.declared_effect_ids || []),
      ...(candidate.runtime_family_effect_ids || []),
      ...(candidate.packet_observed_effect_ids || []),
      ...collectIds(candidate.formula_routes, EFFECT_KEYS),
    ]),
    skill_ids: collectIds(candidate, SKILL_KEYS),
    damage_ids: collectIds(candidate, DAMAGE_KEYS),
    recount_ids: collectIds(candidate, RECOUNT_KEYS),
    attribute_ids: collectIds(candidate, ATTRIBUTE_KEYS),
    class_ids: collectIds(candidate, CLASS_KEYS),
    specialization_ids: collectIds(candidate, SPECIALIZATION_KEYS),
    item_ids: collectIds(candidate, ITEM_KEYS),
    equipment_suit_entries: equipmentSuitEntries(candidate),
    source_config_ids: equipmentSuitEntries(candidate).map(
      (entry) => entry.attribute_key,
    ),
  };
}

function equipmentSuitEntries(candidate) {
  const match = /^equipment-set:(\d+):\d+:variant:(\d+)$/.exec(
    String(candidate.source_id || ""),
  );
  if (!match) return [];
  return [{ map_key: Number(match[1]), attribute_key: Number(match[2]) }];
}

function leaf(value) {
  const selectors = {
    source_rule_ids: uniqueStrings(value.selectors.source_rule_ids),
    effect_ids: uniqueNumbers(value.selectors.effect_ids),
    skill_ids: uniqueNumbers(value.selectors.skill_ids),
    damage_ids: uniqueNumbers(value.selectors.damage_ids),
    recount_ids: uniqueNumbers(value.selectors.recount_ids),
    attribute_ids: uniqueNumbers(value.selectors.attribute_ids),
    class_ids: uniqueNumbers(value.selectors.class_ids),
    specialization_ids: uniqueNumbers(value.selectors.specialization_ids),
    item_ids: uniqueNumbers(value.selectors.item_ids),
    equipment_suit_entries: uniqueEquipmentSuitEntries(
      value.selectors.equipment_suit_entries,
    ),
    source_config_ids: uniqueNumbers(value.selectors.source_config_ids),
  };
  return {
    obligation_id: value.obligation_id,
    domain: value.domain,
    subject_kind: value.subject_kind,
    subject_id: String(value.subject_id),
    subject_name: String(value.subject_name),
    proof_state: "pending-matching-build-validation",
    requirements: uniqueStrings(value.requirements),
    required_event_kinds: value.required_event_kinds
      ? uniqueStrings(value.required_event_kinds)
      : requiredEventKinds(value.requirements, selectors),
    selectors,
    formula_inputs: normalizeFormulaInputs(value.formula_inputs || []),
    evidence: value.evidence,
  };
}

function normalizeFormulaInputs(inputs) {
  return inputs.map((input) => {
    const inputKind = String(input.input_kind || "attribute");
    const candidateAttributeIds = uniqueNumbers(input.candidate_attribute_ids || []);
    const candidateAbilityIds = uniqueNumbers(input.candidate_ability_ids || []);
    const allowedTiers = uniqueNumbers(input.allowed_tiers || []);
    const classAttributeRoutes = normalizeClassAttributeRoutes(input.class_attribute_routes || []);
    const actorRole = String(input.actor_role || "");
    const completion = String(input.completion || (
      inputKind === "loadout_tier"
        ? "exact-current-equipped-tier-observed-before-trigger"
        : inputKind === "class_attribute"
          ? "exact-current-class-selected-value-observed-before-trigger"
        : "any-current-value-observed-before-trigger"
    ));
    const loadoutScope = input.loadout_scope == null
      ? null
      : String(input.loadout_scope);
    const validAttribute = inputKind === "attribute"
      && completion === "any-current-value-observed-before-trigger"
      && candidateAttributeIds.length > 0
      && candidateAbilityIds.length === 0
      && loadoutScope === null
      && allowedTiers.length === 0
      && classAttributeRoutes.length === 0;
    const validLoadoutTier = inputKind === "loadout_tier"
      && completion === "exact-current-equipped-tier-observed-before-trigger"
      && candidateAttributeIds.length === 0
      && candidateAbilityIds.length > 0
      && ["primary", "auxiliary", "any"].includes(loadoutScope)
      && allowedTiers.length > 0
      && classAttributeRoutes.length === 0;
    const validClassAttribute = inputKind === "class_attribute"
      && completion === "exact-current-class-selected-value-observed-before-trigger"
      && candidateAttributeIds.length === 0
      && candidateAbilityIds.length === 0
      && loadoutScope === null
      && allowedTiers.length === 0
      && classAttributeRoutes.length > 0;
    if (!input.input_key || !["source", "target"].includes(actorRole)
      || (!validAttribute && !validLoadoutTier && !validClassAttribute)) {
      throw new Error(`invalid formula input contract: ${JSON.stringify(input)}`);
    }
    return {
      input_key: String(input.input_key),
      label: String(input.label || input.input_key),
      input_kind: inputKind,
      actor_role: actorRole,
      completion,
      candidate_attribute_ids: candidateAttributeIds,
      candidate_ability_ids: candidateAbilityIds,
      ...(loadoutScope === null ? {} : { loadout_scope: loadoutScope }),
      allowed_tiers: allowedTiers,
      class_attribute_routes: classAttributeRoutes,
      evidence: uniqueStrings(input.evidence || []),
    };
  });
}

function normalizeClassAttributeRoutes(routes) {
  const seenClasses = new Set();
  return routes.map((route) => {
    const classIds = uniqueNumbers(route.class_ids || []);
    const candidateAttributeIds = uniqueNumbers(route.candidate_attribute_ids || []);
    if (classIds.length === 0 || candidateAttributeIds.length === 0
      || classIds.some((classId) => seenClasses.has(classId))) {
      throw new Error(`invalid or overlapping class-attribute route: ${JSON.stringify(route)}`);
    }
    classIds.forEach((classId) => seenClasses.add(classId));
    return { class_ids: classIds, candidate_attribute_ids: candidateAttributeIds };
  });
}

function requiredEventKinds(requirements, selectors) {
  const text = requirements.join(" ").toLowerCase();
  const kinds = new Set(["damage"]);
  if (selectors.effect_ids.length > 0 || /status|buff|lifecycle|stack|window/.test(text)) {
    kinds.add("status");
  }
  if (/provider|recipient|owner|class|specialization|summon/.test(text)) kinds.add("actor");
  if (/cast|action|energy|trigger|skill|snapshot/.test(text)) kinds.add("cast");
  if (selectors.attribute_ids.length > 0 || /attribute|stat|mastery|armor|defense|resistance|mitigation|formula|input/.test(text)) {
    kinds.add("entity_attributes");
    kinds.add("temporary_attributes");
  }
  return [...kinds].sort();
}

function buildIndexes(leaves) {
  const indexes = {
    effect_id: {}, skill_id: {}, damage_id: {}, recount_id: {}, attribute_id: {},
    class_id: {}, specialization_id: {}, item_id: {}, source_rule_id: {}, event_kind: {},
    equipment_suit_pair: {}, source_config_id: {},
  };
  const mapping = {
    effect_ids: "effect_id", skill_ids: "skill_id", damage_ids: "damage_id",
    recount_ids: "recount_id", attribute_ids: "attribute_id", class_ids: "class_id",
    specialization_ids: "specialization_id", item_ids: "item_id",
    source_rule_ids: "source_rule_id",
    source_config_ids: "source_config_id",
  };
  for (const leaf of leaves) {
    for (const [selectorKey, indexKey] of Object.entries(mapping)) {
      for (const value of leaf.selectors[selectorKey]) addIndex(indexes[indexKey], value, leaf.obligation_id);
    }
    for (const kind of leaf.required_event_kinds) addIndex(indexes.event_kind, kind, leaf.obligation_id);
    for (const entry of leaf.selectors.equipment_suit_entries) {
      addIndex(
        indexes.equipment_suit_pair,
        `${entry.map_key}:${entry.attribute_key}`,
        leaf.obligation_id,
      );
    }
  }
  return indexes;
}

function addIndex(index, key, obligationId) {
  const normalized = String(key);
  const values = index[normalized] ||= [];
  if (!values.includes(obligationId)) values.push(obligationId);
}

function validateLeaves(leaves) {
  const ids = new Set();
  for (const row of leaves) {
    if (!row.obligation_id || ids.has(row.obligation_id)) {
      throw new Error(`duplicate or empty obligation ID: ${row.obligation_id}`);
    }
    ids.add(row.obligation_id);
    if (!row.domain || !row.subject_id || row.requirements.length === 0 || row.required_event_kinds.length === 0) {
      throw new Error(`incomplete normalized obligation: ${JSON.stringify(row)}`);
    }
    for (const entry of row.selectors.equipment_suit_entries) {
      if (!Number.isSafeInteger(entry.map_key) || entry.map_key <= 0
        || !Number.isSafeInteger(entry.attribute_key) || entry.attribute_key <= 0) {
        throw new Error(`invalid equipment suit selector: ${JSON.stringify(entry)}`);
      }
    }
  }
}

function uniqueEquipmentSuitEntries(values) {
  return [...new Map((values || []).map((entry) => {
    const normalized = {
      map_key: Number(entry.map_key),
      attribute_key: Number(entry.attribute_key),
    };
    return [`${normalized.map_key}:${normalized.attribute_key}`, normalized];
  })).values()].sort(
    (left, right) => left.map_key - right.map_key
      || left.attribute_key - right.attribute_key,
  );
}

function collectIds(value, acceptedKeys) {
  const result = [];
  visit(value, "", acceptedKeys, result);
  return uniqueNumbers(result);
}

function visit(value, key, acceptedKeys, result) {
  if (value === null || value === undefined) return;
  if (acceptedKey(key, acceptedKeys)) {
    const values = Array.isArray(value) ? value : [value];
    for (const item of values) {
      const number = Number(item);
      if (Number.isSafeInteger(number) && number > 0) result.push(number);
    }
  }
  if (Array.isArray(value)) {
    for (const item of value) visit(item, key, acceptedKeys, result);
  } else if (typeof value === "object") {
    for (const [childKey, child] of Object.entries(value)) visit(child, childKey, acceptedKeys, result);
  }
}

function acceptedKey(key, acceptedKeys) {
  if (acceptedKeys.has(key)) return true;
  if (acceptedKeys === ATTRIBUTE_KEYS) return /(?:^|_)attr(?:ibute)?(?:_family)?_ids?$/.test(key);
  if (acceptedKeys === EFFECT_KEYS) return /(?:^|_)(?:effect|buff)_ids?$/.test(key);
  if (acceptedKeys === SKILL_KEYS) return /(?:^|_)(?:skill|ability)_ids?$/.test(key);
  if (acceptedKeys === DAMAGE_KEYS) return /(?:^|_)damage_ids?$/.test(key);
  if (acceptedKeys === RECOUNT_KEYS) return /(?:^|_)recount_ids?$/.test(key);
  if (acceptedKeys === CLASS_KEYS) return /(?:^|_)class_ids?$/.test(key);
  if (acceptedKeys === SPECIALIZATION_KEYS) return /(?:^|_)(?:specialization|spec)_ids?$/.test(key);
  if (acceptedKeys === ITEM_KEYS) return /(?:^|_)item_ids?$/.test(key);
  return false;
}

function countBy(values, selector) {
  return values.reduce((counts, value) => {
    const key = selector(value);
    counts[key] = (counts[key] || 0) + 1;
    return counts;
  }, {});
}

function countMany(values, selector) {
  return values.reduce((counts, value) => {
    for (const key of selector(value)) counts[key] = (counts[key] || 0) + 1;
    return counts;
  }, {});
}

function countIndexValues(leaves, key) {
  return new Set(leaves.flatMap((leaf) => leaf.selectors[key])).size;
}

function assertBuild(value, expected, label, field = "game_build") {
  if (!expected || String(value[field] || "") !== expected) {
    throw new Error(`${label} build ${value[field]} does not match ${expected}`);
  }
}

function assertCount(label, actual, expected) {
  if (!Number.isSafeInteger(Number(expected)) || actual !== Number(expected)) {
    throw new Error(`${label} normalized ${actual}, expected ${expected}`);
  }
}

function artifactReference(filePath) {
  const content = readFileSync(filePath);
  return {
    path: relativeRepoPath(filePath),
    sha256: createHash("sha256").update(content).digest("hex"),
  };
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`${label} could not be read from ${filePath}: ${error.message}`);
  }
}

function uniqueNumbers(values) {
  return [...new Set((values || []).map(Number).filter((value) => Number.isSafeInteger(value) && value > 0))]
    .sort((left, right) => left - right);
}

function uniqueStrings(values) {
  return [...new Set((values || []).map(String).filter(Boolean))].sort();
}

function resolvePath(input) {
  if (!input) throw new Error("a required path was not supplied");
  return path.isAbsolute(input) ? input : path.resolve(repoRoot, input);
}

function relativeRepoPath(input) {
  return path.relative(repoRoot, input).replaceAll("\\", "/");
}

function parseArgs(argv) {
  const result = {};
  const allowed = new Set([
    "factorClosure", "remainingTrace", "globalGate", "targetMitigation", "mastery", "damageStage", "reportSchema", "output", "runtimeOutput",
  ]);
  for (let index = 0; index < argv.length; index += 2) {
    const argument = argv[index];
    const key = argument?.startsWith("--") ? argument.slice(2) : "";
    const value = argv[index + 1];
    if (!allowed.has(key) || !value) throw new Error(`invalid argument: ${argument}`);
    result[key] = value;
  }
  for (const key of allowed) if (!result[key]) throw new Error(`--${key} is required`);
  return result;
}
