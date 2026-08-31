#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

const SKILL_TABLES = new Set([
  "SkillAoyiStarTable", "SkillAoyiTable", "SkillDataTable", "SkillEffectTable", "SkillFightLevelTable",
  "SkillSystemTable", "SkillTable", "SkillUpgradeTable",
]);
const EVENT_ORDER = [
  "actor", "cast", "damage", "status", "entity_attributes", "temporary_attributes", "formula_inputs",
  "profile_selection", "resource", "cooldown", "healing", "shield_state",
];
const SELECTOR_KEYS = [
  "effect_ids", "skill_ids", "damage_ids", "recount_ids", "attribute_ids", "class_ids",
  "specialization_ids", "item_ids", "source_config_ids", "equipment_suit_entries",
];
const VALIDATION_ROUTES = new Set([
  "outgoing-damage", "outgoing-selected-ability-damage", "owned-companion-outgoing-damage",
  "outgoing-healing", "outgoing-shield-or-barrier-state", "named-shield-state",
  "incoming-damage-mitigation", "owned-resource-transition", "selected-ability-cooldown-transition",
  "named-skill-output", "named-status-lifecycle", "named-resource-decay-lifecycle",
]);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "inspect") inspect(path.resolve(required(options, "input")), required(options, "source-rule"));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    batches: path.resolve(required(parsed, "batches")),
    router: path.resolve(required(parsed, "router")),
    primaryAttackRouteProof: path.resolve(required(parsed, "primary-attack-route-proof")),
    masteryRouteProof: path.resolve(required(parsed, "mastery-route-proof")),
    sourceHpRouteProof: path.resolve(required(parsed, "source-hp-route-proof")),
    output: path.resolve(required(parsed, "output")),
    reportSchema: Number(parsed["report-schema"] ?? 10),
  };
}

function build(context) {
  const batches = readBuildArtifact(context.batches, context.build, "game_build", "semantic resolution batches");
  const router = readBuildArtifact(context.router, context.build, "game_build", "proof frontier router");
  const primaryAttackRouteProof = readBuildArtifact(context.primaryAttackRouteProof, context.build, "game_build", "primary-attack runtime route proof");
  const masteryRouteProof = readBuildArtifact(context.masteryRouteProof, context.build, "game_build", "Mastery runtime route proof");
  const sourceHpRouteProof = readBuildArtifact(context.sourceHpRouteProof, context.build, "game_build", "source-HP runtime route proof");
  const routeIndex = indexRouter(router);
  const formulaRouteIndex = indexFormulaRoutes(primaryAttackRouteProof, masteryRouteProof, sourceHpRouteProof);
  const obligations = [];
  const unindexable = [];
  for (const item of batches.work_items ?? []) {
    const routed = routeIndex.get(item.source_rule_id) ?? {};
    for (const componentRoute of componentRoutesFor(item)) {
      const built = buildObligation(item, routed, componentRoute, formulaRouteIndex);
      if (built.indexable) obligations.push(built.obligation);
      else unindexable.push(built.unindexable);
    }
  }
  obligations.sort((a, b) => compareText(a.obligation_id, b.obligation_id));
  unindexable.sort((a, b) => compareText(a.source_rule_id, b.source_rule_id));
  const report = {
    schema_version: 2,
    generated_by: "tools/bpsr-proof-correlation-manifest.mjs",
    game: "blue-protocol-star-resonance",
    game_build: context.build,
    validation_report_schema: context.reportSchema,
    policy: {
      one_capture_scan_answers_all_indexed_frontier_questions: true,
      source_rule_ids_are_metadata_not_runtime_selectors: true,
      unindexable_work_is_preserved_not_silently_skipped: true,
      candidate_sets_remain_bounded_and_non_promotional: true,
      build_mismatch_is_reported_but_does_not_blank_results: true,
      zero_hidden_omissions: true,
    },
    inputs: {
      semantic_resolution_batches: fileDescriptor(context.batches),
      proof_frontier_router: fileDescriptor(context.router),
      primary_attack_runtime_route_proof: fileDescriptor(context.primaryAttackRouteProof),
      mastery_runtime_route_proof: fileDescriptor(context.masteryRouteProof),
      source_hp_runtime_route_proof: fileDescriptor(context.sourceHpRouteProof),
    },
    summary: summarize(batches.work_items ?? [], obligations, unindexable),
    indexes: summarizeIndexes(obligations),
    obligations,
    unindexable_work_items: unindexable,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(context.output);
  console.log(
    `Proof correlation manifest built for ${context.build}: ${obligations.length} indexed obligations, ` +
    `${unindexable.length} explicitly unindexable work items, zero hidden omissions.`,
  );
}

function componentRoutesFor(item) {
  const recipient = item.requirements?.recipient_scope;
  if (recipient?.transfer_gate?.kind !== "component-scoped-routing-only") return [null];
  const routes = recipient.component_routes ?? [];
  return routes.length > 0 ? routes : [null];
}

function buildObligation(item, routed, componentRoute = null, formulaRouteIndex = new Map()) {
  const selectors = emptySelectors();
  const evidenceTables = new Set();
  if (componentRoute) {
    collectComponentSelectors(componentRoute.proof_binding, selectors);
    const selectedIds = new Set(SELECTOR_KEYS.flatMap((key) => selectors[key]).map(String));
    for (const evidence of item.indexed_evidence ?? []) {
      if (!selectedIds.has(String(evidence.id))) continue;
      for (const table of evidence.decoded_tables ?? []) evidenceTables.add(table);
    }
  } else {
    for (const evidence of item.indexed_evidence ?? []) {
      const id = numericId(evidence.id);
      if (!id) continue;
      for (const table of evidence.decoded_tables ?? []) {
        evidenceTables.add(table);
        if (table === "BuffTable") selectors.effect_ids.push(id);
        if (table === "DamageAttrTable") selectors.damage_ids.push(id);
        if (table === "RecountTable") selectors.recount_ids.push(id);
        if (SKILL_TABLES.has(table)) selectors.skill_ids.push(id);
      }
    }
    for (const id of routed.effect_ids ?? []) selectors.effect_ids.push(numericId(id));
    for (const id of routed.candidate_damage_ids ?? []) selectors.damage_ids.push(numericId(id));
    for (const candidate of item.requirements?.produced_damage_route?.candidates ?? []) {
      selectors.recount_ids.push(numericId(candidate.recount_id));
      for (const id of candidate.damage_ids ?? []) selectors.damage_ids.push(numericId(id));
    }
    const ownedOutput = item.requirements?.owned_output_route;
    for (const id of ownedOutput?.effect_ids ?? []) selectors.effect_ids.push(numericId(id));
    for (const id of ownedOutput?.damage_ids ?? []) selectors.damage_ids.push(numericId(id));
    for (const id of ownedOutput?.recount_ids ?? []) selectors.recount_ids.push(numericId(id));
  }
  normalizeSelectors(selectors);
  const requiredEventKinds = eventKinds(item, selectors);
  const requirements = componentRequirements(item, componentRoute);
  const componentKey = componentRoute?.component_key ?? null;
  const transferGate = componentRoute?.transfer_gate ?? item.requirements?.recipient_scope?.transfer_gate;
  const formulaRoute = resolveFormulaRoute(formulaRouteIndex.get(item.source_rule_id), componentKey);
  const numericSelectorCount = SELECTOR_KEYS.reduce((sum, key) => sum + selectors[key].length, 0);
  if (numericSelectorCount === 0) {
    return {
      indexable: false,
      unindexable: {
        work_item_key: componentKey
          ? `frontier:${item.source_rule_id}:component:${slug(componentKey)}`
          : `frontier:${item.source_rule_id}`,
        source_rule_id: item.source_rule_id,
        component_key: componentKey,
        source_id: item.source_id,
        source_name: item.source_name,
        phase: item.phase?.id ?? null,
        reason: componentKey
          ? "No exact canonical numeric selector is present in this component's proof_binding; source-wide selectors are intentionally forbidden."
          : "No proven canonical numeric runtime selector is available; source_rule_id alone is metadata-only.",
        retained_identifiers: unique((item.identifiers ?? []).map(String)),
        decoded_tables: [...evidenceTables].sort(compareText),
        proof_binding: componentRoute?.proof_binding ?? null,
        transfer_gate: normalizeTransferGate(transferGate),
        next_proof_actions: requirements,
      },
    };
  }
  return {
    indexable: true,
    obligation: {
      obligation_id: componentKey
        ? `current-frontier:${item.source_rule_id}:component:${slug(componentKey)}`
        : `current-frontier:${item.source_rule_id}`,
      domain: item.phase?.id ?? "current-build-rdps-proof-frontier",
      subject_kind: item.source_kind ?? item.source_type ?? "unknown",
      subject_id: componentKey
        ? `${String(item.source_id ?? item.source_rule_id)}#${componentKey}`
        : String(item.source_id ?? item.source_rule_id),
      subject_name: componentKey
        ? `${item.source_name ?? String(item.source_id ?? item.source_rule_id)} / ${componentKey}`
        : item.source_name ?? String(item.source_id ?? item.source_rule_id),
      requirements,
      required_event_kinds: requiredEventKinds,
      selectors: { source_rule_ids: [item.source_rule_id], ...selectors },
      formula_inputs: formulaRoute.formula_inputs,
      evidence: {
        validation_route: null,
        component_key: componentKey,
        component_kind: transferGate?.kind ?? null,
        transfer_gate: normalizeTransferGate(transferGate),
        component_scope_route: componentRoute,
        proof_binding: componentRoute?.proof_binding ?? null,
        property_ids: [],
        proof_queue_ids: routed.queue_ids ?? [],
        decoded_tables: [...evidenceTables].sort(compareText),
        proof_input_source: "semantic-resolution-batches.v1",
        owned_output_route: item.requirements?.owned_output_route ?? null,
        formula_input_routes: formulaRoute.formula_input_routes,
        unresolved_formula_input_routes: formulaRoute.unresolved_formula_input_routes,
      },
    },
  };
}

function indexFormulaRoutes(primaryAttackProof, masteryProof, sourceHpProof) {
  const result = new Map();
  const append = (sourceRuleId, value) => {
    const existing = result.get(String(sourceRuleId)) ?? { proven: [], unresolved: [] };
    existing[value.kind].push(value.route);
    result.set(String(sourceRuleId), existing);
  };
  const physicalId = Number(primaryAttackProof.route_contract?.physical_operand_attribute_id);
  const magicalId = Number(primaryAttackProof.route_contract?.magical_operand_attribute_id);
  if (!Number.isSafeInteger(physicalId) || !Number.isSafeInteger(magicalId) || physicalId === magicalId) {
    throw new Error("Primary-attack route proof lacks distinct exact physical/magical attribute ids");
  }
  for (const source of primaryAttackProof.routed_sources ?? []) {
    for (const component of source.components ?? []) {
      const stat = String(component.stat);
      const attributeId = stat === "ATK" ? physicalId : stat === "MATK" ? magicalId : null;
      if (attributeId == null) throw new Error(`Unsupported primary-attack stat ${stat}`);
      append(source.source_rule_id, {
        kind: "proven",
        route: {
          component_key: String(component.component_key),
          input_key: stat === "ATK" ? "physical_attack" : "magical_attack",
          label: stat === "ATK" ? "Physical attack at hit time" : "Magical attack at hit time",
          actor_role: "source",
          completion: "any-current-value-observed-before-trigger",
          candidate_attribute_ids: [attributeId],
          authority: "primary-attack-runtime-route-proof.v1",
        },
      });
    }
  }
  const masteryId = Number(masteryProof.route_contract?.tracked_final_mastery_attribute_id);
  if (!Number.isSafeInteger(masteryId)) throw new Error("Mastery route proof lacks an exact final Mastery attribute id");
  for (const blocker of masteryProof.blocker_obligations ?? []) {
    append(blocker.source_rule_id, {
      kind: "proven",
      route: {
        component_key: componentKeyFromModel(blocker.model_key),
        input_key: "mastery",
        label: "Mastery at hit time",
        actor_role: "source",
        completion: "any-current-value-observed-before-trigger",
        candidate_attribute_ids: [masteryId],
        authority: "mastery-runtime-route-proof.v1",
      },
    });
  }
  for (const blocker of sourceHpProof.blocker_obligations ?? []) {
    append(blocker.source_rule_id, {
      kind: "unresolved",
      route: {
        component_key: componentKeyFromModel(blocker.model_key),
        formula_term: "sourceHpBasis",
        authority: "source-hp-runtime-route-proof.v1",
        reason: "Current-HP versus max-HP selector and coherent hit-time snapshot remain unproven; no candidate attribute is emitted.",
        candidate_attribute_ids: [],
      },
    });
  }
  return result;
}

function componentKeyFromModel(modelKey) {
  const text = String(modelKey ?? "");
  const separator = text.indexOf(":");
  return separator >= 0 ? text.slice(separator + 1) : text;
}

function resolveFormulaRoute(indexed, componentKey) {
  if (!indexed) return { formula_inputs: [], formula_input_routes: [], unresolved_formula_input_routes: [] };
  const matchesComponent = (route) => componentKey == null || slug(route.component_key) === slug(componentKey);
  const proven = (indexed.proven ?? []).filter(matchesComponent);
  const unresolved = (indexed.unresolved ?? []).filter(matchesComponent);
  const deduplicated = new Map();
  for (const route of proven) {
    const key = [
      route.input_key,
      route.input_kind ?? "attribute",
      ...(route.candidate_attribute_ids ?? []),
      "abilities",
      ...(route.candidate_ability_ids ?? []),
      route.loadout_scope ?? "",
      "tiers",
      ...(route.allowed_tiers ?? []),
      "class-routes",
      ...(route.class_attribute_routes ?? []).flatMap((classRoute) => [
        ...(classRoute.class_ids ?? []),
        "attributes",
        ...(classRoute.candidate_attribute_ids ?? []),
      ]),
    ].join(":");
    deduplicated.set(key, route);
  }
  const routes = [...deduplicated.values()].sort((a, b) => compareText(a.input_key, b.input_key));
  return {
    formula_inputs: routes.map(normalizeFormulaInputRoute),
    formula_input_routes: routes,
    unresolved_formula_input_routes: unresolved,
  };
}

function normalizeFormulaInputRoute(route) {
  const inputKind = String(route.input_kind ?? "attribute");
  const classAttributeRoutes = (route.class_attribute_routes ?? []).map((classRoute) => ({
    class_ids: unique((classRoute.class_ids ?? []).map(Number)),
    candidate_attribute_ids: unique((classRoute.candidate_attribute_ids ?? []).map(Number)),
  }));
  return {
    input_key: String(route.input_key),
    label: String(route.label),
    input_kind: inputKind,
    actor_role: String(route.actor_role),
    completion: String(route.completion),
    candidate_attribute_ids: unique((route.candidate_attribute_ids ?? []).map(Number)),
    candidate_ability_ids: unique((route.candidate_ability_ids ?? []).map(Number)),
    ...(route.loadout_scope == null ? {} : { loadout_scope: String(route.loadout_scope) }),
    allowed_tiers: unique((route.allowed_tiers ?? []).map(Number)),
    class_attribute_routes: classAttributeRoutes,
  };
}

function collectComponentSelectors(proofBinding, selectors) {
  if (!proofBinding || typeof proofBinding !== "object") return;
  const visit = (value, keyHint = "") => {
    if (Array.isArray(value)) {
      for (const entry of value) visit(entry, keyHint);
      return;
    }
    if (value && typeof value === "object") {
      for (const [key, entry] of Object.entries(value)) visit(entry, key);
      return;
    }
    const id = numericId(value);
    if (id == null) return;
    const key = String(keyHint).toLowerCase();
    if (key.includes("sourceconfig") || key.includes("sourcenode")) selectors.source_config_ids.push(id);
    else if (key.includes("specialization") || key.includes("spec")) selectors.specialization_ids.push(id);
    else if (key.includes("recount")) selectors.recount_ids.push(id);
    else if (key.includes("damage")) selectors.damage_ids.push(id);
    else if (key.includes("skill")) selectors.skill_ids.push(id);
    else if (key.includes("buff") || key.includes("effect")) selectors.effect_ids.push(id);
    else if (key.includes("attribute")) selectors.attribute_ids.push(id);
    else if (key.includes("class")) selectors.class_ids.push(id);
    else if (key.includes("item")) selectors.item_ids.push(id);
  };
  visit(proofBinding);
}

function componentRequirements(item, componentRoute) {
  const result = flattenRequirements(item);
  if (!componentRoute) return result;
  result.push(...(componentRoute.required_runtime_evidence ?? []).map(String));
  result.push(...(componentRoute.transfer_gate?.required_current_build_evidence ?? []).map(String));
  return unique(result);
}

function indexRouter(router) {
  const result = new Map();
  const dimensions = [router.route_queues, router.formula_queues, router.recipient_queues];
  for (const queues of dimensions) {
    for (const [queueId, queue] of Object.entries(queues ?? {})) {
      for (const item of queue.items ?? []) {
        const existing = result.get(item.source_rule_id) ?? { queue_ids: [], effect_ids: [], candidate_damage_ids: [] };
        existing.queue_ids.push(queueId);
        existing.effect_ids.push(...(item.effect_ids ?? []));
        existing.candidate_damage_ids.push(...(item.candidate_damage_ids ?? []));
        existing.queue_id = existing.queue_ids.sort(compareText).join("+");
        result.set(item.source_rule_id, existing);
      }
    }
  }
  return result;
}

function eventKinds(item, selectors) {
  const kinds = new Set();
  const requirements = item.requirements ?? {};
  if (selectors.skill_ids.length > 0) kinds.add("cast");
  if (selectors.effect_ids.length > 0) kinds.add("status");
  if (selectors.damage_ids.length > 0 || selectors.recount_ids.length > 0 || requirements.produced_damage_route || requirements.owned_output_route || requirements.conservation_replay_required) {
    kinds.add("damage");
  }
  if (requirements.formula) {
    kinds.add("damage");
    kinds.add("entity_attributes");
    kinds.add("temporary_attributes");
    kinds.add("formula_inputs");
  }
  if (requirements.recipient_scope) {
    kinds.add("actor");
    kinds.add("damage");
  }
  if (kinds.size === 0 && selectors.effect_ids.length > 0) kinds.add("status");
  return EVENT_ORDER.filter((kind) => kinds.has(kind));
}

function flattenRequirements(item) {
  const result = [];
  if (item.phase?.proof_gate) result.push(item.phase.proof_gate);
  for (const dependency of item.requirements?.semantic_dependencies ?? []) {
    result.push(`${dependency.kind}: ${dependency.evidence ?? dependency.required_model ?? "proof required"}`);
  }
  const produced = item.requirements?.produced_damage_route;
  if (produced?.next_proof_action) result.push(produced.next_proof_action);
  const ownedOutput = item.requirements?.owned_output_route;
  if (ownedOutput?.next_proof_action) result.push(ownedOutput.next_proof_action);
  const formula = item.requirements?.formula;
  if (formula?.remaining_requirement) result.push(formula.remaining_requirement);
  const recipient = item.requirements?.recipient_scope;
  if (recipient?.remaining_requirement) result.push(recipient.remaining_requirement);
  if (item.requirements?.conservation_replay_required) {
    result.push("Replay the baseline and counterfactual through party conservation before promotion.");
  }
  return unique(result);
}

function summarize(workItems, obligations, unindexable) {
  const requiredEventCounts = {};
  for (const obligation of obligations) {
    for (const kind of obligation.required_event_kinds) requiredEventCounts[kind] = (requiredEventCounts[kind] ?? 0) + 1;
  }
  const coveredSourceRules = new Set([
    ...obligations.flatMap((item) => item.selectors?.source_rule_ids ?? []),
    ...unindexable.map((item) => item.source_rule_id),
  ]);
  const componentRoutesExpected = workItems.reduce(
    (sum, item) => sum + (componentRoutesFor(item)[0] === null ? 0 : componentRoutesFor(item).length),
    0,
  );
  return {
    frontier_work_items: workItems.length,
    covered_frontier_work_items: coveredSourceRules.size,
    indexed_obligations: obligations.length,
    explicitly_unindexable_work_items: unindexable.length,
    explicitly_unindexable_obligations: unindexable.length,
    component_routes_expected: componentRoutesExpected,
    component_obligations: obligations.filter((item) => item.evidence?.component_key).length,
    component_unindexable_obligations: unindexable.filter((item) => item.component_key).length,
    obligations_with_formula_inputs: obligations.filter((item) => (item.formula_inputs ?? []).length > 0).length,
    formula_input_routes: obligations.reduce((sum, item) => sum + (item.formula_inputs ?? []).length, 0),
    obligations_with_unresolved_formula_input_routes: obligations.filter((item) => (item.evidence?.unresolved_formula_input_routes ?? []).length > 0).length,
    required_event_kind_counts: sortObject(requiredEventCounts),
    hidden_omissions: 0,
  };
}

function summarizeIndexes(obligations) {
  const sets = Object.fromEntries(SELECTOR_KEYS.map((key) => [key, new Set()]));
  for (const obligation of obligations) {
    for (const key of SELECTOR_KEYS) for (const value of obligation.selectors[key]) sets[key].add(String(value));
  }
  return Object.fromEntries(SELECTOR_KEYS.map((key) => [key, [...sets[key]].sort(compareNumericText)]));
}

function verify(input) {
  const report = readJson(input, "proof correlation manifest");
  if (report.schema_version !== 2) throw new Error("Proof correlation manifest schema_version must be 2");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Proof correlation manifest content hash mismatch");
  if (!report.policy?.zero_hidden_omissions || !report.policy?.unindexable_work_is_preserved_not_silently_skipped) {
    throw new Error("Proof correlation manifest policy is unsafe");
  }
  const obligationIds = new Set();
  for (const obligation of report.obligations ?? []) {
    if (obligationIds.has(obligation.obligation_id)) throw new Error(`Duplicate obligation ${obligation.obligation_id}`);
    obligationIds.add(obligation.obligation_id);
    if (!Array.isArray(obligation.required_event_kinds) || obligation.required_event_kinds.length === 0) {
      throw new Error(`Obligation ${obligation.obligation_id} has no event kind`);
    }
    if (obligation.evidence?.validation_route != null && !VALIDATION_ROUTES.has(obligation.evidence.validation_route)) {
      throw new Error(`Obligation ${obligation.obligation_id} has an unsupported Rust validation route`);
    }
    if (!obligation.evidence?.transfer_gate?.kind || obligation.evidence.transfer_gate.kind !== obligation.evidence.component_kind) {
      throw new Error(`Obligation ${obligation.obligation_id} is missing its authoritative typed transfer gate`);
    }
    const selectorCount = SELECTOR_KEYS.reduce((sum, key) => sum + (obligation.selectors?.[key]?.length ?? 0), 0);
    if (selectorCount === 0) throw new Error(`Obligation ${obligation.obligation_id} has no numeric selector`);
    for (const key of SELECTOR_KEYS.filter((candidate) => candidate !== "equipment_suit_entries")) {
      for (const value of obligation.selectors?.[key] ?? []) {
        if (!Number.isSafeInteger(value)) {
          throw new Error(`Obligation ${obligation.obligation_id} selector ${key} must contain Rust-compatible JSON integers`);
        }
      }
    }
    for (const value of obligation.selectors?.equipment_suit_entries ?? []) {
      if (!Number.isSafeInteger(value?.map_key) || !Number.isSafeInteger(value?.attribute_key)) {
        throw new Error(`Obligation ${obligation.obligation_id} equipment_suit_entries must contain integer map_key and attribute_key values`);
      }
    }
    for (const input of obligation.formula_inputs ?? []) {
      if (!String(input.input_key ?? "").trim() || !String(input.label ?? "").trim()) throw new Error(`Obligation ${obligation.obligation_id} has an unnamed formula input`);
      const inputKind = String(input.input_kind ?? "attribute");
      const attributes = input.candidate_attribute_ids ?? [];
      const abilities = input.candidate_ability_ids ?? [];
      const tiers = input.allowed_tiers ?? [];
      const classRoutes = input.class_attribute_routes ?? [];
      const validAttribute = inputKind === "attribute"
        && input.completion === "any-current-value-observed-before-trigger"
        && Array.isArray(attributes) && attributes.length > 0
        && attributes.every(Number.isSafeInteger)
        && Array.isArray(abilities) && abilities.length === 0
        && input.loadout_scope == null
        && Array.isArray(tiers) && tiers.length === 0
        && Array.isArray(classRoutes) && classRoutes.length === 0;
      const validLoadoutTier = inputKind === "loadout_tier"
        && input.completion === "exact-current-equipped-tier-observed-before-trigger"
        && Array.isArray(attributes) && attributes.length === 0
        && Array.isArray(abilities) && abilities.length > 0
        && abilities.every(Number.isSafeInteger)
        && ["primary", "auxiliary", "any"].includes(input.loadout_scope)
        && Array.isArray(tiers) && tiers.length > 0
        && tiers.every((tier) => Number.isSafeInteger(tier) && tier > 0)
        && Array.isArray(classRoutes) && classRoutes.length === 0;
      const routedClasses = classRoutes.flatMap((route) => route?.class_ids ?? []);
      const validClassAttribute = inputKind === "class_attribute"
        && input.completion === "exact-current-class-selected-value-observed-before-trigger"
        && Array.isArray(attributes) && attributes.length === 0
        && Array.isArray(abilities) && abilities.length === 0
        && input.loadout_scope == null
        && Array.isArray(tiers) && tiers.length === 0
        && Array.isArray(classRoutes) && classRoutes.length > 0
        && classRoutes.every((route) => Array.isArray(route?.class_ids) && route.class_ids.length > 0
          && route.class_ids.every((classId) => Number.isSafeInteger(classId) && classId > 0)
          && Array.isArray(route?.candidate_attribute_ids) && route.candidate_attribute_ids.length > 0
          && route.candidate_attribute_ids.every((attributeId) => Number.isSafeInteger(attributeId) && attributeId > 0))
        && new Set(routedClasses).size === routedClasses.length;
      if (!["source", "target"].includes(input.actor_role) || (!validAttribute && !validLoadoutTier && !validClassAttribute)) throw new Error(`Obligation ${obligation.obligation_id} has an unsupported formula-input route`);
    }
    if ((obligation.formula_inputs ?? []).length > 0 && !obligation.required_event_kinds.includes("formula_inputs")) throw new Error(`Obligation ${obligation.obligation_id} has formula inputs without the formula_inputs event kind`);
  }
  const coveredSourceRules = new Set([
    ...(report.obligations ?? []).flatMap((item) => item.selectors?.source_rule_ids ?? []),
    ...(report.unindexable_work_items ?? []).map((item) => item.source_rule_id),
  ]);
  if (coveredSourceRules.size !== Number(report.summary?.frontier_work_items)) {
    throw new Error(`Frontier work-item omission detected: ${coveredSourceRules.size}`);
  }
  if (coveredSourceRules.size !== Number(report.summary?.covered_frontier_work_items)) {
    throw new Error("Covered frontier work-item summary is inconsistent");
  }
  const componentTotal = (report.obligations ?? []).filter((item) => item.evidence?.component_key).length
    + (report.unindexable_work_items ?? []).filter((item) => item.component_key).length;
  if (componentTotal !== Number(report.summary?.component_routes_expected)) {
    throw new Error(`Component-route omission detected: ${componentTotal}`);
  }
  if (Number(report.summary?.hidden_omissions) !== 0) throw new Error("Hidden omissions must remain zero");
  console.log(
    `Proof correlation manifest verified for ${report.game_build}: ${report.obligations.length} indexed, ` +
    `${report.unindexable_work_items.length} unindexable, zero omissions.`,
  );
  return report;
}

function inspect(input, sourceRuleId) {
  const report = verify(input);
  const obligations = report.obligations.filter((item) => item.selectors?.source_rule_ids?.includes(sourceRuleId));
  const unindexable = report.unindexable_work_items.filter((item) => item.source_rule_id === sourceRuleId);
  if (obligations.length === 0 && unindexable.length === 0) throw new Error(`Unknown source rule ${sourceRuleId}`);
  const result = obligations.length + unindexable.length === 1
    ? obligations[0] ?? unindexable[0]
    : { obligations, unindexable_work_items: unindexable };
  console.log(JSON.stringify(result, null, 2));
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-proof-correlation-manifest-"));
  try {
    const batches = path.join(root, "batches.json");
    const router = path.join(root, "router.json");
    const output = path.join(root, "manifest.json");
    const primaryAttackRouteProof = path.join(root, "primary-attack.json");
    const masteryRouteProof = path.join(root, "mastery.json");
    const sourceHpRouteProof = path.join(root, "source-hp.json");
    writeJson(batches, {
      game_build: "123", work_items: [
        fixture("buff", "11", ["BuffTable"], null),
        fixture("route", "12", ["TalentTable"], { candidates: [{ recount_id: "14", damage_ids: ["15"] }] }, null, {
          effect_ids: [17], damage_ids: [18], recount_ids: [19], transfer_credit_eligible: false,
          next_proof_action: "verify owner output",
        }),
        fixture("unindexable", "13", ["TalentTable"], null),
        fixture("mixed", "1501", ["TalentTable"], null, {
          transfer_gate: transferGate("component-scoped-routing-only", "component route decides attribution"),
          component_routes: [
            componentFixture(
              "target-vulnerability",
              { originSourceConfigId: 3003041, runtimeBuffId: 3003012, semanticBuffId: 3003010, sourceNodeId: 1501 },
              "external-recipient-counterfactual",
            ),
            componentFixture(
              "attack-stat-reduction",
              { originSourceConfigId: 3003041, runtimeBuffId: 3003012, sourceNodeId: 1501 },
              "non-outgoing-damage-component",
            ),
            componentFixture(
              "movement-speed-reduction",
              { originSourceConfigId: 3003041, runtimeBuffId: 3003014, sourceNodeId: 1501 },
              "non-outgoing-damage-component",
            ),
          ],
        }),
      ],
    });
    writeJson(router, {
      game_build: "123",
      route_queues: { runtime_candidate_correlation: { items: [{ source_rule_id: "mrs:route", candidate_damage_ids: ["16"] }] } },
      formula_queues: { current_build_targeted_observation: { items: [{ source_rule_id: "mrs:buff", effect_ids: ["11"] }] } },
      recipient_queues: {},
    });
    writeJson(primaryAttackRouteProof, {
      game_build: "123",
      route_contract: { physical_operand_attribute_id: 11330, magical_operand_attribute_id: 11340 },
      routed_sources: [{ source_rule_id: "mrs:buff", components: [{ component_key: "atk", stat: "ATK" }] }],
    });
    writeJson(masteryRouteProof, {
      game_build: "123",
      route_contract: { tracked_final_mastery_attribute_id: 11940 },
      blocker_obligations: [{ source_rule_id: "mrs:route", model_key: "runtime-input:mastery" }],
    });
    writeJson(sourceHpRouteProof, {
      game_build: "123",
      blocker_obligations: [{ source_rule_id: "mrs:mixed", model_key: "runtime-input:target-vulnerability" }],
    });
    build({ build: "123", batches, router, primaryAttackRouteProof, masteryRouteProof, sourceHpRouteProof, output, reportSchema: 10 });
    const report = verify(output);
    if (report.obligations.length !== 5 || report.unindexable_work_items.length !== 1) throw new Error("Manifest self-test counts failed");
    if (report.summary.frontier_work_items !== 4 || report.summary.component_routes_expected !== 3 || report.summary.component_obligations !== 3) {
      throw new Error("Manifest self-test component coverage failed");
    }
    if (
      !report.indexes.damage_ids.includes("16")
      || !report.indexes.damage_ids.includes("18")
      || !report.indexes.recount_ids.includes("14")
      || !report.indexes.recount_ids.includes("19")
      || !report.indexes.effect_ids.includes("17")
    ) throw new Error("Manifest self-test indexes failed");
    const ownedOutput = report.obligations.find((item) => item.selectors.source_rule_ids.includes("mrs:route"))?.evidence?.owned_output_route;
    if (ownedOutput?.transfer_credit_eligible !== false) throw new Error("Manifest self-test lost source-owned output semantics");
    if (!report.obligations.every((item) => SELECTOR_KEYS.filter((key) => key !== "equipment_suit_entries").every((key) => (item.selectors[key] ?? []).every(Number.isSafeInteger)))) {
      throw new Error("Manifest self-test Rust-compatible selector types failed");
    }
    if (!report.obligations.every((item) => item.evidence?.transfer_gate?.kind)) {
      throw new Error("Manifest self-test did not preserve typed transfer gates");
    }
    if (report.summary.obligations_with_formula_inputs !== 2 || report.summary.formula_input_routes !== 2) throw new Error("Manifest self-test formula input routing failed");
    if (!report.obligations.find((item) => item.selectors.source_rule_ids.includes("mrs:buff"))?.formula_inputs.some((input) => input.candidate_attribute_ids.includes(11330))) throw new Error("Manifest self-test lost the physical attack input route");
    if (!report.obligations.find((item) => item.selectors.source_rule_ids.includes("mrs:route"))?.formula_inputs.some((input) => input.candidate_attribute_ids.includes(11940))) throw new Error("Manifest self-test lost the Mastery input route");
    const loadoutTier = normalizeFormulaInputRoute({
      input_key: "imagine_tier",
      label: "Imagine tier at activation",
      input_kind: "loadout_tier",
      actor_role: "source",
      completion: "exact-current-equipped-tier-observed-before-trigger",
      candidate_ability_ids: [3971],
      loadout_scope: "primary",
      allowed_tiers: [0, 1, 2, 3, 4, 5],
    });
    if (loadoutTier.input_kind !== "loadout_tier" || loadoutTier.candidate_ability_ids[0] !== 3971 || loadoutTier.candidate_attribute_ids.length !== 0 || loadoutTier.allowed_tiers.length !== 6 || loadoutTier.allowed_tiers[0] !== 0) throw new Error("Manifest self-test lost the exact loadout-tier input contract");
    const classAttribute = normalizeFormulaInputRoute({
      input_key: "recipient-attack",
      label: "Recipient class-selected attack",
      input_kind: "class_attribute",
      actor_role: "target",
      completion: "exact-current-class-selected-value-observed-before-trigger",
      class_attribute_routes: [
        { class_ids: [1, 11], candidate_attribute_ids: [11330] },
        { class_ids: [2, 13], candidate_attribute_ids: [11340] },
      ],
    });
    if (classAttribute.input_kind !== "class_attribute" || classAttribute.class_attribute_routes.length !== 2 || classAttribute.class_attribute_routes[1].candidate_attribute_ids[0] !== 11340) throw new Error("Manifest self-test lost the class-selected attribute input contract");
    const target = report.obligations.find((item) => item.evidence?.component_key === "target-vulnerability");
    const movement = report.obligations.find((item) => item.evidence?.component_key === "movement-speed-reduction");
    if (!target?.selectors.effect_ids.includes(3003012) || !target.selectors.effect_ids.includes(3003010)) {
      throw new Error("Manifest self-test lost the target-vulnerability component selectors");
    }
    if (target.selectors.effect_ids.includes(3003014) || !movement?.selectors.effect_ids.includes(3003014)) {
      throw new Error("Manifest self-test leaked selectors between component obligations");
    }
    if (target.evidence.transfer_gate.kind === movement.evidence.transfer_gate.kind) {
      throw new Error("Manifest self-test collapsed distinct component transfer gates");
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
  console.log("bpsr-proof-correlation-manifest self-test passed");
}

function fixture(id, numeric, tables, produced, recipientOverride = null, ownedOutput = null) {
  return {
    source_rule_id: `mrs:${id}`, source_id: `talent:${numeric}`, source_name: id, source_kind: "talent", source_type: "talent",
    phase: { id: "runtime-counterfactual-conservation", proof_gate: "prove" },
    requirements: {
      semantic_dependencies: [],
      produced_damage_route: produced,
      owned_output_route: ownedOutput,
      formula: { remaining_requirement: "formula" },
      recipient_scope: recipientOverride ?? {
        transfer_gate: {
          kind: id === "buff" ? "external-recipient-counterfactual" : "source-owned-output-nontransfer",
          attribution_route: id === "buff" ? "provider -> recipient" : "output -> source owner",
          runtime_credit_allowed: false,
          required_current_build_evidence: ["matching-build proof"],
          forbidden_transfers: ["unproved transfer"],
        },
      },
      conservation_replay_required: true,
    },
    identifiers: [numeric], indexed_evidence: [{ id: numeric, decoded_tables: tables }],
  };
}

function componentFixture(componentKey, proofBinding, gateKind) {
  return {
    component_key: componentKey,
    proof_binding: proofBinding,
    required_runtime_evidence: [`prove ${componentKey}`],
    transfer_gate: transferGate(gateKind, `${componentKey} attribution`),
  };
}

function transferGate(kind, attributionRoute) {
  return {
    kind,
    attribution_route: attributionRoute,
    runtime_credit_allowed: false,
    required_current_build_evidence: ["matching-build proof"],
    forbidden_transfers: ["unproved transfer"],
  };
}

function normalizeTransferGate(gate) {
  if (!gate || typeof gate !== "object") return null;
  return {
    kind: gate.kind ?? null,
    attribution_route: gate.attribution_route ?? null,
    authority: gate.authority ?? null,
    runtime_credit_allowed: gate.runtime_credit_allowed === true,
    required_current_build_evidence: unique((gate.required_current_build_evidence ?? []).map(String)),
    forbidden_transfers: unique((gate.forbidden_transfers ?? []).map(String)),
  };
}

function emptySelectors() { return Object.fromEntries(SELECTOR_KEYS.map((key) => [key, []])); }
function normalizeSelectors(selectors) {
  for (const key of SELECTOR_KEYS) {
    const retained = selectors[key].filter((value) => value !== null && value !== undefined);
    if (key === "equipment_suit_entries") {
      selectors[key] = [...new Map(retained.map((value) => [JSON.stringify(value), value])).values()]
        .sort((a, b) => (a.map_key - b.map_key) || (a.attribute_key - b.attribute_key));
    } else {
      selectors[key] = unique(retained).sort(compareNumericText);
    }
  }
}
function numericId(value) {
  const text = String(value ?? "");
  if (!/^\d+$/.test(text)) return null;
  const numeric = Number(text);
  if (!Number.isSafeInteger(numeric)) throw new Error(`Numeric selector ${text} exceeds JavaScript's exact integer range`);
  return numeric;
}
function slug(value) {
  return String(value ?? "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "") || "component";
}
function unique(values) { return [...new Set(values)]; }
function compareText(a, b) { return String(a ?? "").localeCompare(String(b ?? "")); }
function compareNumericText(a, b) { return BigInt(a) < BigInt(b) ? -1 : BigInt(a) > BigInt(b) ? 1 : 0; }
function sortObject(value) { return Object.fromEntries(Object.entries(value).sort(([a], [b]) => compareText(a, b))); }
function contentHash(report) { const clone = structuredClone(report); delete clone.content_sha256; return createHash("sha256").update(JSON.stringify(clone)).digest("hex"); }
function fileDescriptor(file) { const bytes = readFileSync(file); return { name: path.basename(file), bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") }; }
function readBuildArtifact(file, buildId, key, label) { const value = readJson(file, label); if (String(value[key]) !== buildId) throw new Error(`${label} build mismatch`); return value; }
function readJson(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); return JSON.parse(readFileSync(file, "utf8")); }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return String(value[key]); }
function parseArgs(args) { const output = {}; for (let index = 0; index < args.length; index += 2) { const token = args[index]; if (!token?.startsWith("--")) throw new Error(`Unexpected argument ${token}`); const next = args[index + 1]; if (!next || next.startsWith("--")) throw new Error(`Missing value for ${token}`); output[token.slice(2)] = next; } return output; }
function usage(exitCode) { console.log(`Usage:\n  node tools/bpsr-proof-correlation-manifest.mjs build --build <id> --batches <json> --router <json> --primary-attack-route-proof <json> --mastery-route-proof <json> --source-hp-route-proof <json> --output <json> [--report-schema 10]\n  node tools/bpsr-proof-correlation-manifest.mjs verify --input <json>\n  node tools/bpsr-proof-correlation-manifest.mjs inspect --input <json> --source-rule <id>\n  node tools/bpsr-proof-correlation-manifest.mjs self-test`); process.exit(exitCode); }
