#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";

const ROOT = process.cwd();
const BUILD = "24687926";
const RESEARCH = path.join(
  ROOT,
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global",
  `steam-${BUILD}`,
);
const DEFAULT_EXTERNAL = path.join(
  ROOT,
  "..",
  "evidence",
  `RLogs-proof-output-${BUILD}`,
  "all-current-build-effects.external-frontier.current.json",
);
const DEFAULT_OUTPUT = path.join(RESEARCH, "rdps-exhaustive-party-route-ledger.v1.json");

const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  args.set(process.argv[index], process.argv[index + 1]);
}

const externalPath = path.resolve(args.get("--external-frontier") ?? DEFAULT_EXTERNAL);
const outputPath = path.resolve(args.get("--output") ?? DEFAULT_OUTPUT);
if (fs.existsSync(outputPath) && !args.has("--replace")) {
  throw new Error(`refusing to overwrite ${outputPath}; pass --replace yes`);
}

const aoyiPath = path.join(RESEARCH, "current-aoyi-rdps-origin-ledger.candidate.json");
const partyPath = path.join(RESEARCH, "party-skill-static-closure.v1.json");
const classificationPath = path.join(
  ROOT,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-effect-classification.v1.json",
);
const runtimePath = path.join(
  ROOT,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-formula-runtime.v1.json",
);
const buffTablePath = path.join(ROOT, "Excels/BuffTable.json");
const runtimeSourcePaths = [
  path.join(ROOT, "plugins/games/blue-protocol-star-resonance/src/state_rdps.rs"),
  path.join(ROOT, "plugins/games/blue-protocol-star-resonance/src/rdps_runtime.rs"),
  path.join(ROOT, "plugins/games/blue-protocol-star-resonance/src/rdps.rs"),
  path.join(ROOT, "plugins/games/blue-protocol-star-resonance/src/combat_presentation.rs"),
];

const aoyi = readJson(aoyiPath);
const party = readJson(partyPath);
const external = readJson(externalPath);
const classification = readJson(classificationPath);
const runtime = readJson(runtimePath);
const buffTable = readJson(buffTablePath);
const runtimeSourceOriginal = new Map(runtimeSourcePaths.map((file) => [
  file,
  fs.readFileSync(file, "utf8"),
]));
const runtimeSourceText = new Map([...runtimeSourceOriginal].map(([file, text]) => [
  file,
  text.replaceAll("_", ""),
]));

assertIdentity(aoyi.game_build, "Aoyi origin ledger");
assertIdentity(party.game_build, "party-skill closure");
assertIdentity(external.game_build, "external-effect frontier");
assertIdentity(classification.game_build, "runtime classification");
assertIdentity(runtime.game_build, "runtime formula");

const productionByParent = new Map([
  [3903, { effect_ids: [2110065], mechanic: "party-attack-percent" }],
  [3921, { effect_ids: [2110034], mechanic: "party-cooldown-opportunity" }],
  [3935, { effect_ids: [2110096], mechanic: "party-triggered-produced-damage" }],
  [3942, { effect_ids: [2110099], mechanic: "stacking-party-target-vulnerability" }],
  [3957, { effect_ids: [2110125], mechanic: "party-all-element-bonus" }],
  [3971, { effect_ids: [2110140], mechanic: "party-main-stat" }],
  [3974, { effect_ids: [2110143], mechanic: "party-attack-percent" }],
  [3982, {
    effect_ids: [2110167],
    mechanic: "separated-party-target-vulnerability-component",
    remaining_obligation: "the simultaneous Element Resistance reduction component remains fail closed until the matching target resistance family, source penetration/ignore state, overlap order, caps, and integer rounding are reconstructable around the installed 11000 transform",
  }],
]);

const openOffensiveParents = new Map([
  [3934, {
    effect_ids: [2110078],
    mechanic: "enemy-armor-reduction",
    disposition: "candidate-fail-closed",
    remaining_obligation: "use exact status-source ownership plus equipped item 3000035 and tier as the runtime selector, then reconstruct the normal ATK/MATK defense-affected subtotal, source/target penetration overlap, operation order, rounding, and conserved marginal around the installed 22000 transform",
  }],
  [3914, {
    effect_ids: [2110092],
    mechanic: "enemy-armor-reduction",
    disposition: "candidate-fail-closed",
    remaining_obligation: "reconstruct the normal ATK/MATK defense-affected subtotal, source/target penetration overlap, operation order, rounding, and conserved marginal around the installed 22000 transform",
  }],
  [3946, {
    effect_ids: [2110092],
    mechanic: "conditional-shared-projectile-enemy-armor-reduction",
    disposition: "candidate-fail-closed",
    remaining_obligation: "select the actual provider from the shared projectile status lifecycle, then reconstruct the normal ATK/MATK defense-affected subtotal, source/target penetration overlap, operation order, rounding, and conserved marginal around the installed 22000 transform",
  }],
]);

const nonDamagePartyParents = new Map([
  [3908, "party-healing-and-dispel"],
  [3911, "party-death-prevention"],
  [3920, "party-revival"],
  [3951, "party-mitigation; bonus damage belongs to the caster"],
  [3962, "party-healing; produced damage belongs to the caster"],
  [3981, "party-shield"],
]);

const hostileOrNonSupportParents = new Map([
  [3961, "skill can damage allies; it does not create allied outgoing damage"],
]);

const reviewedComponentDispositions = new Map([
  [2110033, "Time Decree short-lived search/emitter marker; no recipient damage component"],
  [2110064, "Fiery Battle Will source/emitter config; child 2110065 owns the exact external Attack lane"],
  [2110151, "Precision Burst area emitter; child 2110143 owns the exact external Attack lane"],
  [2110153, "Precision Burst self-effect multiplier marker; never transfers credit to another player"],
  [2110161, "Celestial Spirit Mage owner transformation marker; no recipient damage component"],
  [2110166, "Celestial Guardian target-finder/emitter marker; recipient debuff 2110167 owns the offensive components"],
  [3200038, "Superconductor Surge passive/support root; exact external offensive child 2110140 is separate"],
  [3210050, "Blazing Axe owner passive/root marker; exact external offensive child 2110065 is separate"],
  [3210210, "Precision Burst owner passive/root marker; exact external offensive child 2110143 is separate"],
  [3210211, "Eye Power increases this Battle Imagine's passive damage and remains ordinary owner output"],
]);

const candidateRemainingObligations = new Map([
  [997520, "reconstruct the 15-second +100% Combat Resource Acquisition opportunity lane with exact gain, cap/overcap, spend, downstream extra-action selection, provider overlap, and conserved counterfactual scheduling"],
  [2110060, "resolve the numeric Haste magnitude absent from all installed tables and decoded skill logic, then model the 10-second field, 10-second linger, nonstacking provider arbitration, downstream action opportunities, and conservation"],
]);

const provenEffectIds = new Set([
  31602, 55228, 55333, 997511, 997513, 997515, 997518, 997534, 997538,
  997570, 998542, 2100154, 2110034, 2110065, 2110096, 2110099, 2110125, 2110140,
  2110143, 2110167, 2202041, 2204471, 2207252, 2302121, 2302421, 2404261, 2404271, 3003052,
  3003411,
]);

const classificationById = new Map(
  classification.effects.map((row) => [Number(row.effect_id), row]),
);
const externalById = new Map(external.effects.map((row) => [Number(row.effect_id), row]));
const buffById = new Map(Object.values(buffTable).map((row) => [Number(row.Id), row]));

const parentRows = aoyi.skills.map((skill) => {
  const skillId = Number(skill.skill_id);
  const production = productionByParent.get(skillId);
  const open = openOffensiveParents.get(skillId);
  const nonDamage = nonDamagePartyParents.get(skillId);
  const hostile = hostileOrNonSupportParents.get(skillId);
  let disposition;
  let mechanic;
  let runtimeEffectIds = [];
  let remainingObligation = null;

  if (production) {
    disposition = "production-enabled";
    mechanic = production.mechanic;
    runtimeEffectIds = production.effect_ids;
    remainingObligation = production.remaining_obligation ?? null;
  } else if (open) {
    disposition = open.disposition;
    mechanic = open.mechanic;
    runtimeEffectIds = open.effect_ids;
    remainingObligation = open.remaining_obligation;
  } else if (nonDamage) {
    disposition = "non-contributing-party-support";
    mechanic = nonDamage;
  } else if (hostile) {
    disposition = "non-contributing-not-allied-support";
    mechanic = hostile;
  } else {
    disposition = "non-contributing-owner-output-or-owner-only-state";
    mechanic = "direct caster output or description-scoped owner state";
  }

  return {
    skill_id: skillId,
    name: skill.name,
    item_id: skill.item_id,
    monster_id: skill.monster_id,
    description: skill.english_description,
    recipient_evidence: skill.recipient_evidence,
    discovered_candidate_classes: skill.candidate_classes,
    reviewed_disposition: disposition,
    reviewed_mechanic: mechanic,
    runtime_effect_ids: runtimeEffectIds,
    remaining_obligation: remainingObligation,
    exact_relationship_count: skill.exact_relationship_candidates?.length ?? 0,
    exact_damage_chain_count: skill.exact_damage_chain_candidates?.length ?? 0,
    descendant_candidate_ids: descendantIdsForSkill(skill),
    source_row_sha256: sha256(skill),
  };
});

const knownParentIds = new Set(parentRows.map((row) => row.skill_id));
for (const id of [
  ...productionByParent.keys(),
  ...openOffensiveParents.keys(),
  ...nonDamagePartyParents.keys(),
  ...hostileOrNonSupportParents.keys(),
]) {
  if (!knownParentIds.has(id)) throw new Error(`reviewed parent ${id} is absent from the Aoyi ledger`);
}

const descendantLinks = new Map();
for (const skill of aoyi.skills) {
  const parentId = Number(skill.skill_id);
  for (const effectId of skill.passive_owner_buff_ids ?? []) {
    addDescendantLink(descendantLinks, effectId, parentId, "passive-owner-id", true);
  }
  for (const candidate of skill.owner_family_candidates ?? []) {
    addDescendantLink(
      descendantLinks,
      candidate.buff_id,
      parentId,
      `owner-family-${candidate.owner_match_strength}`,
      false,
    );
  }
  for (const candidate of skill.semantic_owner_candidates ?? []) {
    addDescendantLink(
      descendantLinks,
      candidate.buff_id ?? candidate.effect_id ?? candidate.id,
      parentId,
      "semantic-owner-candidate",
      false,
    );
  }
  for (const relationship of skill.exact_relationship_candidates ?? []) {
    for (const effectId of [
      ...(relationship.runtime_buff_ids ?? []),
      ...(relationship.source_buff_ids ?? []),
      ...(relationship.historical_effects ?? []).map((row) => row.effect_id),
    ]) {
      addDescendantLink(descendantLinks, effectId, parentId, "exact-relationship-edge", true);
    }
  }
}

for (const [parentId, open] of openOffensiveParents) {
  for (const effectId of open.effect_ids) {
    addDescendantLink(descendantLinks, effectId, parentId, "reviewed-component-route", true);
  }
}

for (const [parentId, production] of productionByParent) {
  for (const effectId of production.effect_ids) {
    addDescendantLink(descendantLinks, effectId, parentId, "production-component-route", true);
  }
}

const descendantRows = [...descendantLinks.entries()]
  .sort(([left], [right]) => left - right)
  .map(([effectId, links]) => {
    const parents = uniqueSorted(links.map((link) => link.parent_skill_id));
    const exact = links.some((link) => link.exact);
    const production = provenEffectIds.has(effectId);
    const open = [...openOffensiveParents.entries()].filter(([, value]) =>
      value.effect_ids.includes(effectId));
    const discoveryEvidence = discoveryEvidenceForEffect(effectId);
    let disposition;
    let remainingObligation = null;
    let dispositionEvidence = null;
    if (production) {
      disposition = "production-enabled";
      dispositionEvidence = "exact effect ID is enabled by the build-24687926 specialized runtime";
    } else if (reviewedComponentDispositions.has(effectId)) {
      disposition = "non-contributing-reviewed-component";
      remainingObligation = null;
      dispositionEvidence = reviewedComponentDispositions.get(effectId);
    } else if (open.length > 0) {
      disposition = "candidate-fail-closed";
      remainingObligation = [...new Set(open.map(([, row]) => row.remaining_obligation))].join("; ");
      dispositionEvidence = `installed-build description identifies ${[...new Set(open.map(([, row]) => row.mechanic))].join(" and ")}`;
    } else if (!exact) {
      disposition = "discovery-only-no-exact-aoyi-owner-edge-current-build";
      remainingObligation = null;
      dispositionEvidence = "retained broad/strong/semantic name-family discovery row; the exhaustive installed-build relationship inventory has no exact numeric Aoyi owner edge, so this is not treated as a runtime contribution route";
    } else {
      const parentDispositions = parents.map((id) =>
        parentRows.find((row) => row.skill_id === id)?.reviewed_disposition);
      disposition = parentDispositions.every((value) =>
        value?.startsWith("non-contributing"))
        ? "non-contributing-through-reviewed-parent"
        : "exact-origin-formula-or-scope-open-fail-closed";
      dispositionEvidence = disposition.startsWith("non-contributing")
        ? "exact owner edge terminates at a parent whose installed-build description is non-contributing"
        : "exact owner edge exists, but the offensive component or counterfactual remains unresolved";
      if (!disposition.startsWith("non-contributing")) {
        remainingObligation = "resolve exact mechanic component, recipient scope, and formula lane";
      }
    }
    return {
      effect_id: effectId,
      parent_skill_ids: parents,
      links,
      exact_owner_edge_present: exact,
      runtime_classification: classificationById.get(effectId) ?? null,
      observed_external_frontier: externalById.get(effectId) ?? null,
      installed_build_buff: installedBuffEvidence(effectId),
      discovery_evidence: discoveryEvidence,
      reviewed_disposition: disposition,
      disposition_evidence: dispositionEvidence,
      remaining_obligation: remainingObligation,
    };
  });

const aoyiComponentRouteRows = aoyi.skills.flatMap((skill) =>
  (skill.component_routes ?? []).map((route) => {
    const healingOnly = route.component_id === "healing-bomb-recount-sibling-healing";
    return {
      parent_skill_id: Number(skill.skill_id),
      parent_skill_name: skill.name,
      component_id: route.component_id,
      role: route.role,
      effect_ids: numericIds(route.effect_ids),
      source_config_ids: numericIds(route.source_config_ids),
      recipient_scope: route.recipient_scope,
      source_disposition: route.rdps_disposition,
      current_runtime_production_effect_ids: numericIds(route.effect_ids)
        .filter((effectId) => provenEffectIds.has(effectId)),
      reviewed_disposition: healingOnly
        ? "healing-output-noncontributing-to-rdps"
        : route.rdps_disposition,
      proof_state: route.proof_state,
      remaining_obligation: route.component_id === "celestial-guardian-morale-reduction"
        ? "the exact Vulnerability component is production-enabled; retain only the separate Element Resistance reduction component fail closed until the matching target resistance family, source penetration/ignore state, overlap order, caps, and integer rounding are reconstructable around the installed 11000 transform"
        : ((!healingOnly && (
          /block|preserve-never-transfer|counterfactual-only|credit-only/i
            .test(route.rdps_disposition ?? "")
          || numericIds(route.effect_ids).some((effectId) =>
            classificationById.get(effectId)?.review_state === "candidate")
        ) && !numericIds(route.effect_ids).some((effectId) => provenEffectIds.has(effectId)))
          ? "retain this exact component route fail closed until its source disposition's named runtime/formula/conservation gate is closed"
          : null),
      source_row_sha256: sha256(route),
    };
  }));

const aoyiDamageRouteRows = aoyi.skills.flatMap((skill) =>
  (skill.exact_damage_chain_candidates ?? []).flatMap((group) => {
    const directIds = new Set(numericIds(group.damage_ids));
    const sourceTargetIds = new Set(numericIds(group.source_target_damage_ids));
    return uniqueSorted([...directIds, ...sourceTargetIds]).map((damageId) => {
      const componentRoutes = (skill.component_routes ?? [])
        .filter((route) => numericIds(route.effect_ids).includes(damageId));
      const sourceDispositions = [...new Set(componentRoutes
        .map((route) => route.rdps_disposition)
        .filter(Boolean))];
      const productionEffectId = damageId === 2211009603 ? 2110096 : null;
      const unresolved = productionEffectId === null
        && sourceDispositions.some((value) =>
          /block|preserve-never-transfer|counterfactual-only|credit-only/i.test(value));
      const damageRow = [...(group.damage_attr_rows ?? []),
        ...(group.source_target_damage_attr_rows ?? [])]
        .find((row) => Number(row.Id) === damageId) ?? null;
      return {
        parent_skill_id: Number(skill.skill_id),
        parent_skill_name: skill.name,
        skill_effect_id: Number(group.skill_effect_id),
        damage_id: damageId,
        route_roles: [
          ...(directIds.has(damageId) ? ["skill-effect-damage-id"] : []),
          ...(sourceTargetIds.has(damageId) ? ["source-target-damage-id"] : []),
        ],
        component_ids: componentRoutes.map((route) => route.component_id),
        source_dispositions: sourceDispositions,
        reviewed_disposition: productionEffectId !== null
          ? `production-enabled-via-effect-${productionEffectId}`
          : sourceDispositions.length > 0
            ? sourceDispositions.join("; ")
            : "ordinary-owner-damage-route-no-support-transfer",
        remaining_obligation: unresolved
          ? "retain the exact damage route fail closed until its linked component route is resolved"
          : null,
        installed_build_damage: damageRow === null ? null : {
          id: Number(damageRow.Id),
          name: damageRow.Name ?? null,
          type_enum: damageRow.TypeEnum ?? null,
          damage_script: damageRow.DamageScript ?? null,
          damage_type: damageRow.DamageType ?? null,
          damage_property: damageRow.DamageProperty ?? null,
          coefficient_basis_points: damageRow.PVEDamageRadio ?? [],
          fixed_parameters: damageRow.PVEFixedParameter ?? [],
        },
        source_group_sha256: sha256(group),
      };
    });
  }));

const partySkillRows = party.skill_candidates.map((row) => ({
  skill_id: Number(row.skill_id),
  name: row.localized_name_evidence ?? row.design_name_evidence,
  description: row.description_evidence,
  support_categories: row.support_categories,
  rdps_relevant_candidate: row.rdps_relevant_candidate,
  exact_reviewed_buff_or_status_ids: numericIds(row.exact_reviewed_buff_or_status_ids),
  exact_skill_to_buff_ids: numericIds(row.exact_skill_to_buff_edges),
  candidate_skill_to_buff_ids: numericIds(row.reviewed_candidate_skill_to_buff_links),
  graph_state: row.skill_to_buff_graph_state,
  provider_rdps_credit_allowed: row.provider_rdps_credit_allowed,
  reviewed_disposition: row.rdps_relevant_candidate
    ? "candidate-route-retained-for-exact-effect-review"
    : "non-contributing-static-scope-review",
  source_row_sha256: row.row_sha256,
}));

const partyBuffRows = party.buff_candidates.map((row) => ({
  effect_id: Number(row.buff_id),
  level: row.level,
  name: row.localized_name_evidence ?? row.design_name_evidence,
  description: row.description_evidence,
  support_categories: row.support_categories,
  rdps_relevant_candidate: row.rdps_relevant_candidate,
  runtime_classification: classificationById.get(Number(row.buff_id)) ?? null,
  observed_external_frontier: externalById.get(Number(row.buff_id)) ?? null,
  installed_build_buff: installedBuffEvidence(Number(row.buff_id)),
  production_enabled: provenEffectIds.has(Number(row.buff_id)),
  provider_rdps_credit_allowed: row.provider_rdps_credit_allowed,
  reviewed_disposition: provenEffectIds.has(Number(row.buff_id))
    ? "production-enabled"
    : classificationById.has(Number(row.buff_id))
      ? `runtime-${classificationById.get(Number(row.buff_id)).review_state}`
      : row.rdps_relevant_candidate
        ? "candidate-fail-closed"
        : "non-contributing-static-scope-review",
  source_row_sha256: row.row_sha256,
}));

const rogueRows = party.rogue_party_entry_candidates.map((row) => ({
  entry_id: Number(row.entry_id),
  name: row.localized_name_evidence ?? row.design_name_evidence,
  support_categories: row.support_categories,
  rdps_relevant_candidate: row.rdps_relevant_candidate,
  root_effect_id: Number(row.exact_root_buff_id),
  child_effect_ids: numericIds(row.candidate_child_buff_family),
  provider_rdps_credit_allowed: row.provider_rdps_credit_allowed,
  reviewed_disposition: row.rdps_relevant_candidate
    || numericIds(row.candidate_child_buff_family).some((effectId) =>
      classificationById.get(effectId)?.review_state === "candidate")
    ? "candidate-child-family-fail-closed"
    : "non-contributing-static-scope-review",
  source_row_sha256: row.row_sha256,
}));

const externalRows = external.effects.map((row) => ({
  effect_id: Number(row.effect_id),
  display_name: row.display_name,
  primary_model_family: row.primary_model_family,
  model_families: row.model_families,
  proof_queue: row.current_proof_queue,
  frontier_promotion_state: row.promotion_state,
  frontier_damage_disposition: row.current_damage_disposition,
  runtime_classification: classificationById.get(Number(row.effect_id)) ?? null,
  production_enabled: provenEffectIds.has(Number(row.effect_id)),
  reviewed_disposition: provenEffectIds.has(Number(row.effect_id))
    ? "production-enabled"
    : classificationById.get(Number(row.effect_id))?.review_state ?? "unclassified-fail-closed",
  proof_gates: row.proof_gates,
}));

const allEffectIds = new Set([
  ...provenEffectIds,
  ...descendantRows.map((row) => row.effect_id),
  ...partyBuffRows.map((row) => row.effect_id),
  ...externalRows.map((row) => row.effect_id),
  ...classification.effects.map((row) => Number(row.effect_id)),
  ...rogueRows.flatMap((row) => [row.root_effect_id, ...row.child_effect_ids]),
  ...partySkillRows.flatMap((row) => [
    ...row.exact_reviewed_buff_or_status_ids,
    ...row.exact_skill_to_buff_ids,
    ...row.candidate_skill_to_buff_ids,
  ]),
]);
const consolidatedEffectRows = [...allEffectIds]
  .filter((effectId) => Number.isSafeInteger(effectId) && effectId > 0)
  .sort((left, right) => left - right)
  .map((effectId) => {
    const descendant = descendantRows.find((row) => row.effect_id === effectId) ?? null;
    const partyBuff = partyBuffRows.find((row) => row.effect_id === effectId) ?? null;
    const observed = externalRows.find((row) => row.effect_id === effectId) ?? null;
    const runtimeClassification = classificationById.get(effectId) ?? null;
    const productionEnabled = provenEffectIds.has(effectId);
    const disposition = productionEnabled
      ? "production-enabled"
      : runtimeClassification
        ? `runtime-${runtimeClassification.review_state}`
        : descendant?.reviewed_disposition
          ?? partyBuff?.reviewed_disposition
          ?? "retained-unclassified-fail-closed";
    return {
      effect_id: effectId,
      installed_build_buff: installedBuffEvidence(effectId),
      present_in: {
        production_frontier: productionEnabled,
        aoyi_descendants: descendant !== null,
        party_buff_closure: partyBuff !== null,
        observed_external_frontier: observed !== null,
        runtime_classification: runtimeClassification !== null,
        rogue_party_graph: rogueRows.some((row) =>
          row.root_effect_id === effectId || row.child_effect_ids.includes(effectId)),
      },
      routes: {
        aoyi_parent_links: descendant?.links ?? [],
        party_skill_ids: partySkillRows
          .filter((row) => [
            ...row.exact_reviewed_buff_or_status_ids,
            ...row.exact_skill_to_buff_ids,
            ...row.candidate_skill_to_buff_ids,
          ].includes(effectId))
          .map((row) => row.skill_id),
        rogue_entry_ids: rogueRows
          .filter((row) => row.root_effect_id === effectId || row.child_effect_ids.includes(effectId))
          .map((row) => row.entry_id),
      },
      runtime_classification: runtimeClassification,
      production_enabled: productionEnabled,
      reviewed_disposition: disposition,
      remaining_obligation: descendant?.remaining_obligation
        ?? candidateRemainingObligations.get(effectId)
        ?? (disposition.includes("candidate") || disposition.includes("unclassified")
          ? "retain exact identity and resolve the missing formula/scope/origin gate before attribution"
          : null),
    };
  });

const activeParameterEvidenceByEffect = new Map();
for (const skill of aoyi.skills) {
  for (const parameter of skill.active_modifier_parameter_evidence ?? []) {
    for (const effectId of numericIds(parameter.active_effect_ids)) {
      const rows = activeParameterEvidenceByEffect.get(effectId) ?? [];
      rows.push({
        parent_skill_id: Number(skill.skill_id),
        parent_skill_name: skill.name,
        skill_effect_id: Number(parameter.skill_effect_id),
        semantic_labels: parameter.semantic_labels ?? [],
        parameter_encoding: parameter.parameter_encoding ?? null,
        raw_units_per_percent: parameter.raw_units_per_percent ?? null,
        raw_units_per_decimal: parameter.raw_units_per_decimal ?? null,
        duration_seconds: parameter.duration_seconds ?? null,
        tiers: parameter.tiers ?? [],
        proof_state: parameter.proof_state ?? null,
      });
      activeParameterEvidenceByEffect.set(effectId, rows);
    }
  }
}

const componentRoutesByEffect = new Map();
for (const row of aoyiComponentRouteRows) {
  for (const effectId of row.effect_ids) {
    const rows = componentRoutesByEffect.get(effectId) ?? [];
    rows.push(row);
    componentRoutesByEffect.set(effectId, rows);
  }
}

const runtimeEvidenceCache = new Map();
const exactIdRouteRows = [];
for (const effect of consolidatedEffectRows) {
  addExactRoute(effect.effect_id, "canonical-effect", `effect:${effect.effect_id}`, {
    origin: {
      source: "reconciled-union",
      present_in: effect.present_in,
    },
  });
}
for (const descendant of descendantRows) {
  for (const link of descendant.links) {
    addExactRoute(descendant.effect_id, "aoyi-parent-link",
      `aoyi-parent:${link.parent_skill_id}:${link.source}`, {
        origin: {
          parent_skill_id: link.parent_skill_id,
          parent_skill_name: parentRows.find((row) => row.skill_id === link.parent_skill_id)?.name ?? null,
          relationship_source: link.source,
          exact_numeric_owner_edge: link.exact,
        },
        route_disposition: descendant.reviewed_disposition,
        route_remaining_obligation: descendant.remaining_obligation,
      });
  }
}
for (const component of aoyiComponentRouteRows) {
  for (const effectId of component.effect_ids) {
    addExactRoute(effectId, "aoyi-component",
      `aoyi-component:${component.parent_skill_id}:${component.component_id}`, {
        origin: {
          parent_skill_id: component.parent_skill_id,
          parent_skill_name: component.parent_skill_name,
          component_id: component.component_id,
          source_config_ids: component.source_config_ids,
          proof_state: component.proof_state,
        },
        providerScope: "owner of the exact parent skill or packet-resolved child source",
        recipientScope: component.recipient_scope,
        route_disposition: component.reviewed_disposition,
        route_remaining_obligation: component.remaining_obligation,
      });
  }
}
for (const damage of aoyiDamageRouteRows) {
  addExactRoute(damage.damage_id, "aoyi-damage-output",
    `aoyi-damage:${damage.parent_skill_id}:${damage.skill_effect_id}:${damage.damage_id}`, {
      origin: {
        parent_skill_id: damage.parent_skill_id,
        parent_skill_name: damage.parent_skill_name,
        skill_effect_id: damage.skill_effect_id,
        route_roles: damage.route_roles,
        component_ids: damage.component_ids,
        installed_build_damage: damage.installed_build_damage,
        source_group_sha256: damage.source_group_sha256,
      },
      providerScope: "owner of the exact parent skill or its packet-resolved summon",
      recipientScope: "enemy target of the exact damage output",
      route_disposition: damage.reviewed_disposition,
      route_remaining_obligation: damage.remaining_obligation,
    });
}
for (const row of partyBuffRows) {
  addExactRoute(row.effect_id, "party-buff-closure", `party-buff:${row.effect_id}`, {
    origin: {
      closure_source: "installed-build party buff semantic inventory",
      support_categories: row.support_categories,
      source_row_sha256: row.source_row_sha256,
    },
    route_disposition: row.reviewed_disposition,
  });
}
for (const skill of partySkillRows) {
  const bindings = [
    ...skill.exact_reviewed_buff_or_status_ids.map((effectId) => [effectId, "reviewed"]),
    ...skill.exact_skill_to_buff_ids.map((effectId) => [effectId, "exact"]),
    ...skill.candidate_skill_to_buff_ids.map((effectId) => [effectId, "candidate"]),
  ];
  for (const [effectId, binding] of bindings) {
    addExactRoute(effectId, "party-skill-binding", `party-skill:${skill.skill_id}:${binding}`, {
      origin: {
        skill_id: skill.skill_id,
        skill_name: skill.name,
        binding_state: binding,
        graph_state: skill.graph_state,
        source_row_sha256: skill.source_row_sha256,
      },
      providerScope: "player owning the exact installed-build skill",
      route_disposition: skill.reviewed_disposition,
    });
  }
}
for (const rogue of rogueRows) {
  addExactRoute(rogue.root_effect_id, "rogue-entry-root", `rogue-entry:${rogue.entry_id}:root`, {
    origin: {
      rogue_entry_id: rogue.entry_id,
      rogue_entry_name: rogue.name,
      relationship: "root-effect",
      child_effect_ids: rogue.child_effect_ids,
      source_row_sha256: rogue.source_row_sha256,
    },
    providerScope: "player owning the exact Rogue entry",
    route_disposition: rogue.reviewed_disposition,
  });
  for (const effectId of rogue.child_effect_ids) {
    addExactRoute(effectId, "rogue-entry-child", `rogue-entry:${rogue.entry_id}:child:${effectId}`, {
      origin: {
        rogue_entry_id: rogue.entry_id,
        rogue_entry_name: rogue.name,
        relationship: "child-effect",
        root_effect_id: rogue.root_effect_id,
        source_row_sha256: rogue.source_row_sha256,
      },
      providerScope: "player owning the exact Rogue entry",
      route_disposition: rogue.reviewed_disposition,
    });
  }
}
for (const observed of externalRows) {
  addExactRoute(observed.effect_id, "observed-external-frontier",
    `observed-external:${observed.effect_id}`, {
      origin: {
        display_name: observed.display_name,
        primary_model_family: observed.primary_model_family,
        model_families: observed.model_families,
        proof_queue: observed.proof_queue,
        proof_gates: observed.proof_gates,
      },
      route_disposition: observed.reviewed_disposition,
    });
}
for (const classificationRow of classification.effects) {
  const effectId = Number(classificationRow.effect_id);
  addExactRoute(effectId, "runtime-classification", `runtime-classification:${effectId}`, {
    origin: {
      source: "rdps-effect-classification.v1.json",
      review_state: classificationRow.review_state,
    },
    providerScope: classificationRow.source_scope,
    recipientScope: classificationRow.target_scope,
    route_disposition: provenEffectIds.has(effectId)
      ? "production-enabled"
      : `runtime-${classificationRow.review_state}`,
  });
}
for (const effectId of provenEffectIds) {
  addExactRoute(effectId, "production-runtime", `production-runtime:${effectId}`, {
    origin: {
      source: "specialized build-24687926 state projector",
      production_frontier: true,
    },
    route_disposition: "production-enabled",
  });
}
exactIdRouteRows.sort((left, right) => left.effect_id - right.effect_id
  || left.route_key.localeCompare(right.route_key));

const broadOwnerIds = new Set(
  aoyi.skills.flatMap((skill) => (skill.owner_family_candidates ?? [])
    .filter((row) => row.owner_match_strength === "broad")
    .map((row) => Number(row.buff_id))),
);
const originalAoyiDescendantIds = new Set(
  aoyi.skills.flatMap((skill) => descendantIdsForSkill(skill)),
);
const productionEffectEvidenceGaps = [...provenEffectIds]
  .sort((left, right) => left - right)
  .flatMap((effectId) => {
    const evidence = runtimeEvidenceForEffect(effectId);
    const missing = [];
    if (evidence.source_files_containing_exact_id.length === 0) missing.push("runtime-source");
    if (evidence.test_functions.length === 0) missing.push("exact-id-test");
    const compiledCatalogBinding = evidence.source_files_containing_exact_id.includes(
      "plugins/games/blue-protocol-star-resonance/src/state_rdps.rs",
    ) && evidence.test_functions.some((test) =>
      test.file === "plugins/games/blue-protocol-star-resonance/src/state_rdps.rs");
    if (evidence.runtime_config_locations.length === 0
      && evidence.runtime_reference_locations.length === 0
      && !compiledCatalogBinding) {
      missing.push("runtime-config-or-compiled-catalog-binding");
    }
    return missing.length > 0 ? [{ effect_id: effectId, missing }] : [];
  });

assertCount(parentRows.length, 73, "Aoyi parent skills");
assertCount(broadOwnerIds.size, 108, "broad owner-family descendants");
assertCount(originalAoyiDescendantIds.size, 214, "original Aoyi descendant union");
assertCount(descendantRows.length, 218, "reconciled Aoyi descendants");
assertCount(partySkillRows.length, 56, "party skill rows");
assertCount(partyBuffRows.length, 101, "party buff rows");
assertCount(rogueRows.length, 22, "rogue party rows");
assertCount(externalRows.length, 124, "observed external effects");
assertCount(classification.effects.length, 266, "runtime classification effects");
assertCount(aoyiComponentRouteRows.length, 52, "Aoyi component routes");
assertCount(aoyiDamageRouteRows.length, 136, "Aoyi parent-to-damage-ID routes");
assertCount(provenEffectIds.size, 29, "production effect IDs");
assertCount(productionEffectEvidenceGaps.length, 0,
  "production effect IDs missing runtime source, exact-ID tests, or runtime bindings");
assertCount(
  externalRows.filter((row) => row.runtime_classification === null).length,
  0,
  "unclassified observed external effects",
);
assertCount(parentRows.filter((row) => row.reviewed_disposition === "production-enabled").length, 8,
  "production Aoyi parents");
assertCount(parentRows.filter((row) => row.reviewed_disposition === "candidate-fail-closed").length, 3,
  "open offensive Aoyi parents");
assertCount(
  descendantRows.filter((row) =>
    row.reviewed_disposition === "discovery-only-no-exact-aoyi-owner-edge-current-build").length,
  127,
  "discovery-only Aoyi rows without an exact owner edge",
);
assertCount(
  consolidatedEffectRows.filter((row) => row.production_enabled).length,
  provenEffectIds.size,
  "production effect IDs represented in consolidated ledger",
);
assertCount(
  consolidatedEffectRows.filter((row) => row.reviewed_disposition === "runtime-candidate").length,
  4,
  "remaining candidate effect IDs",
);
assertCount(
  new Set(exactIdRouteRows.map((row) => row.route_key)).size,
  exactIdRouteRows.length,
  "unique exact effect route keys",
);
assertCount(exactIdRouteRows.length, 1586, "flattened exact ID/route rows");
assertCount(new Set(exactIdRouteRows.map((row) => row.exact_id)).size, 660,
  "unique exact IDs represented by flattened routes");
const missingCanonicalExactRouteIds = consolidatedEffectRows
  .filter((effect) => !exactIdRouteRows.some((row) =>
    row.effect_id === effect.effect_id && row.route_kind === "canonical-effect"))
  .map((row) => row.effect_id);
assertCount(missingCanonicalExactRouteIds.length, 0, "effects missing canonical exact route rows");
const requiredExactRouteFields = [
  "exact_id", "id_kind", "effect_id", "route_key", "route_kind", "origin", "localization", "provider_scope",
  "recipient_scope", "magnitude", "stacking", "lifecycle", "operation_order", "aliases",
  "reviewed_disposition", "remaining_obligation", "runtime", "tests", "source_row_sha256",
];
const incompleteExactRouteRows = exactIdRouteRows.filter((row) =>
  requiredExactRouteFields.some((field) => !Object.hasOwn(row, field)
    || row[field] === undefined
    || (field !== "remaining_obligation" && row[field] === null)));
assertCount(incompleteExactRouteRows.length, 0, "incomplete exact effect route rows");
assertCount(
  exactIdRouteRows.filter((row) => row.reviewed_disposition === null).length,
  0,
  "exact effect routes without dispositions",
);
assertCount(
  aoyiComponentRouteRows.filter((component) => component.effect_ids.some((effectId) =>
    !exactIdRouteRows.some((row) => row.route_key
      === `${effectId}:aoyi-component:${component.parent_skill_id}:${component.component_id}`)))
    .length,
  0,
  "Aoyi component routes missing flattened exact ID rows",
);
assertCount(
  aoyiDamageRouteRows.filter((damage) => !exactIdRouteRows.some((row) => row.route_key
    === `${damage.damage_id}:aoyi-damage:${damage.parent_skill_id}:${damage.skill_effect_id}:${damage.damage_id}`))
    .length,
  0,
  "Aoyi damage routes missing flattened exact ID rows",
);

const report = {
  schema_version: 1,
  generated_by: "tools/bpsr-exhaustive-party-route-ledger.mjs",
  game: "blue-protocol-star-resonance",
  game_build: BUILD,
  policy: {
    exact_numeric_ids_authoritative: true,
    localized_descriptions_are_mechanic_and_scope_evidence_not_runtime_keys: true,
    aggregate_counts_never_substitute_for_row_disposition: true,
    unknown_origin_links_are_retained: true,
    unsupported_provider_credit_fails_closed: true,
    ordinary_damage_and_dps_unchanged: true,
  },
  inputs: [
    aoyiPath,
    partyPath,
    externalPath,
    classificationPath,
    runtimePath,
    buffTablePath,
    ...runtimeSourcePaths,
  ]
    .map(fileIdentity),
  reconciliation: {
    aoyi_parent_skills: parentRows.length,
    aoyi_broad_owner_family_descendants: broadOwnerIds.size,
    aoyi_original_descendant_union: originalAoyiDescendantIds.size,
    aoyi_reconciled_descendants_including_reviewed_component_routes: descendantRows.length,
    aoyi_exact_component_routes: aoyiComponentRouteRows.length,
    aoyi_component_route_effect_ids: new Set(
      aoyiComponentRouteRows.flatMap((row) => row.effect_ids)).size,
    aoyi_parent_to_damage_id_routes: aoyiDamageRouteRows.length,
    aoyi_unique_damage_route_ids: new Set(aoyiDamageRouteRows.map((row) => row.damage_id)).size,
    remaining_aoyi_component_route_ids: aoyiComponentRouteRows
      .filter((row) => row.remaining_obligation !== null)
      .map((row) => `${row.parent_skill_id}:${row.component_id}`),
    remaining_aoyi_damage_route_ids: aoyiDamageRouteRows
      .filter((row) => row.remaining_obligation !== null)
      .map((row) => `${row.parent_skill_id}:${row.damage_id}`),
    party_skill_candidates: partySkillRows.length,
    party_buff_candidates: partyBuffRows.length,
    rogue_party_entry_candidates: rogueRows.length,
    observed_external_effects: externalRows.length,
    runtime_classification_effects: classification.effects.length,
    consolidated_unique_effect_ids: consolidatedEffectRows.length,
    exact_id_route_rows: exactIdRouteRows.length,
    exact_id_route_unique_ids: new Set(exactIdRouteRows.map((row) => row.exact_id)).size,
    production_effect_ids: provenEffectIds.size,
    production_effect_ids_missing_runtime_source_tests_or_config:
      productionEffectEvidenceGaps,
    zero_production_effect_ids_missing_runtime_source_tests_or_config:
      productionEffectEvidenceGaps.length === 0,
    production_aoyi_parents: 8,
    open_offensive_aoyi_parents: 3,
    open_offensive_aoyi_parent_skill_ids: [...openOffensiveParents.keys()].sort((left, right) => left - right),
    open_offensive_aoyi_effect_ids: uniqueSorted(
      [...openOffensiveParents.values()].flatMap((row) => row.effect_ids)
        .filter((effectId) => !reviewedComponentDispositions.has(effectId)),
    ),
    remaining_candidate_effect_ids: consolidatedEffectRows
      .filter((row) => row.reviewed_disposition === "runtime-candidate")
      .map((row) => row.effect_id),
    discovery_only_aoyi_rows_without_exact_owner_edge: descendantRows.filter((row) =>
      row.reviewed_disposition === "discovery-only-no-exact-aoyi-owner-edge-current-build").length,
    zero_discovery_only_rows_missing_evidence: descendantRows
      .filter((row) =>
        row.reviewed_disposition === "discovery-only-no-exact-aoyi-owner-edge-current-build")
      .every((row) => row.discovery_evidence.length > 0),
    zero_unreviewed_aoyi_parents: parentRows.every((row) => row.reviewed_disposition !== null),
    zero_component_routes_without_disposition: aoyiComponentRouteRows.every((row) =>
      row.reviewed_disposition !== null),
    zero_damage_routes_without_disposition: aoyiDamageRouteRows.every((row) =>
      row.reviewed_disposition !== null),
    zero_unclassified_observed_external_effects: externalRows.every((row) =>
      row.runtime_classification !== null),
    zero_effect_rows_without_disposition: consolidatedEffectRows.every((row) =>
      row.reviewed_disposition !== null),
    zero_exact_id_route_rows_without_required_fields: incompleteExactRouteRows.length === 0,
    zero_exact_id_route_rows_without_disposition: exactIdRouteRows.every((row) =>
      row.reviewed_disposition !== null),
    zero_aoyi_component_routes_missing_exact_id_rows: true,
    zero_aoyi_damage_routes_missing_exact_id_rows: true,
    zero_hidden_rows: [...allEffectIds].every((effectId) =>
      consolidatedEffectRows.some((row) => row.effect_id === effectId)),
  },
  aoyi_parent_rows: parentRows,
  aoyi_descendant_rows: descendantRows,
  aoyi_component_route_rows: aoyiComponentRouteRows,
  aoyi_damage_route_rows: aoyiDamageRouteRows,
  party_skill_rows: partySkillRows,
  party_buff_rows: partyBuffRows,
  rogue_party_entry_rows: rogueRows,
  observed_external_effect_rows: externalRows,
  consolidated_effect_rows: consolidatedEffectRows,
  exact_id_route_rows: exactIdRouteRows,
};

report.content_sha256 = sha256(report);
fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(`wrote ${outputPath}`);
console.log(JSON.stringify(report.reconciliation, null, 2));

function addExactRoute(effectId, routeKind, routeIdentity, options = {}) {
  const numericEffectId = Number(effectId);
  const effect = consolidatedEffectRows.find((row) => row.effect_id === numericEffectId) ?? {
    effect_id: numericEffectId,
    installed_build_buff: installedBuffEvidence(numericEffectId),
    present_in: {},
    routes: {
      aoyi_parent_links: [],
      party_skill_ids: [],
      rogue_entry_ids: [],
    },
    runtime_classification: classificationById.get(numericEffectId) ?? null,
    production_enabled: provenEffectIds.has(numericEffectId),
    reviewed_disposition: options.route_disposition ?? "exact-route-retained-fail-closed",
    remaining_obligation: options.route_remaining_obligation ?? null,
  };
  const routeKey = `${Number(effectId)}:${routeIdentity}`;
  if (exactIdRouteRows.some((row) => row.route_key === routeKey)) {
    throw new Error(`duplicate exact effect route ${routeKey}`);
  }
  const classificationRow = effect.runtime_classification;
  const installed = effect.installed_build_buff;
  const componentRoutes = componentRoutesByEffect.get(Number(effectId)) ?? [];
  const activeParameters = activeParameterEvidenceByEffect.get(Number(effectId)) ?? [];
  const runtimeEvidence = runtimeEvidenceForEffect(Number(effectId));
  const routeDisposition = options.route_disposition ?? effect.reviewed_disposition;
  const remainingObligation = options.route_remaining_obligation !== undefined
    ? options.route_remaining_obligation
    : effect.remaining_obligation;
  exactIdRouteRows.push({
    exact_id: Number(effectId),
    id_kind: routeKind === "aoyi-damage-output"
      ? "damage-output-id"
      : installed !== null || classificationRow !== null
        ? "effect-or-status-id"
        : "route-bound-config-attribute-or-output-id",
    effect_id: Number(effectId),
    route_key: routeKey,
    route_kind: routeKind,
    origin: options.origin ?? { source: routeKind },
    localization: {
      design_name: installed?.design_name ?? null,
      localized_name: installed?.localized_name ?? null,
      localized_description: installed?.localized_description ?? null,
      contextual_parent_names: uniqueStrings([
        ...effect.routes.aoyi_parent_links.map((link) =>
          parentRows.find((row) => row.skill_id === link.parent_skill_id)?.name),
        ...effect.routes.party_skill_ids.map((skillId) =>
          partySkillRows.find((row) => row.skill_id === skillId)?.name),
        ...effect.routes.rogue_entry_ids.map((entryId) =>
          rogueRows.find((row) => row.entry_id === entryId)?.name),
      ]),
      placeholder_localization_retained: installed !== null
        && installed.localized_name === "气刃突刺计数",
    },
    provider_scope: options.providerScope
      ?? classificationRow?.source_scope
      ?? (componentRoutes.length > 0
        ? "owner of the exact component source"
        : "unresolved; provider credit fails closed"),
    recipient_scope: options.recipientScope
      ?? classificationRow?.target_scope
      ?? uniqueStrings(componentRoutes.map((row) => row.recipient_scope)),
    magnitude: {
      classification_basis_points: classificationRow?.magnitude_basis_points ?? null,
      active_parameter_ladders: activeParameters,
      runtime_config_locations: runtimeEvidence.runtime_config_locations,
      exact_numeric_magnitude_resolved: classificationRow?.magnitude_basis_points !== null
        && classificationRow?.magnitude_basis_points !== undefined
        || activeParameters.length > 0
        || effect.production_enabled,
    },
    stacking: {
      installed_repeat_add_rule: installed?.stacking_rule ?? null,
      reviewed_runtime_rule: classificationRow?.stacking_rule ?? null,
    },
    lifecycle: {
      installed_destroy_rules: installed?.duration_rules ?? null,
      exact_status_level: installed?.level ?? null,
      lifecycle_identity_present: installed !== null,
    },
    operation_order: operationOrderForEffect(Number(effectId), classificationRow),
    aliases: {
      aoyi_parent_links: effect.routes.aoyi_parent_links,
      party_skill_ids: effect.routes.party_skill_ids,
      rogue_entry_ids: effect.routes.rogue_entry_ids,
      component_ids: uniqueStrings(componentRoutes.map((row) => row.component_id)),
    },
    reviewed_disposition: routeDisposition,
    remaining_obligation: remainingObligation,
    runtime: {
      production_enabled: effect.production_enabled,
      generic_classification_attribution_enabled: classificationRow?.attribution_enabled ?? false,
      specialized_runtime_owned: effect.production_enabled
        && classificationRow?.attribution_enabled !== true,
      source_files_containing_exact_id: runtimeEvidence.source_files_containing_exact_id,
      runtime_config_locations: runtimeEvidence.runtime_config_locations,
      runtime_reference_locations: runtimeEvidence.runtime_reference_locations,
      unsupported_or_ambiguous_fails_closed: !effect.production_enabled
        || remainingObligation !== null,
    },
    tests: {
      exact_id_test_functions: runtimeEvidence.test_functions,
      production_contract: effect.production_enabled
        ? [
          "ordinary Damage and DPS remain unchanged",
          "exact rDMG conserves by provider/effect/recipient",
          "shared live and history projector paths agree",
          "malformed, ambiguous, expired, gap-crossing, or stale state fails closed",
        ]
        : [
          "unsupported route emits zero transferred damage",
          "exact identity remains visible in canonical state and presentation",
        ],
      focused_source: "plugins/games/blue-protocol-star-resonance/src/state_rdps.rs",
      full_suite_command: "cargo test -p rlogs-game-bpsr --lib",
    },
    source_row_sha256: sha256({
      effect_id: Number(effectId),
      route_kind: routeKind,
      route_identity: routeIdentity,
      origin: options.origin ?? null,
    }),
  });
}

function operationOrderForEffect(effectId, classificationRow) {
  if (effectId === 2110078 || effectId === 2110092) {
    return {
      lane: "target-defense-transform-before-packet-final-damage",
      authority: false,
      note: "The installed 22,000 defense transform is known, but the normal ATK/MATK-only affected subtotal, overlap, and integer order are not yet reconstructable for runtime transfer.",
    };
  }
  const kind = classificationRow?.contribution_kind ?? null;
  const lanes = {
    target_vulnerability: "additive-target-vulnerability-stage",
    direct_damage_amplification: "attacker-damage-amplification-stage",
    offensive_stat_boost: "recipient-stat-stage-before-dependent-damage-consumer",
    haste: "action-opportunity-stage-before-per-hit-damage-modifiers",
    resource_support: "resource-opportunity-stage-before-action-selection",
    state_scaling: "consumer-specific-state-scaling-stage",
    healing_support: "healing-only-no-outgoing-damage-stage",
    mitigation: "defensive-only-no-outgoing-damage-stage",
    environmental: "environmental-no-player-provider-stage",
    internal_marker: "routing-or-lifecycle-only-no-damage-stage",
    self_only: "owner-only-damage-context-never-transferred",
  };
  return {
    lane: lanes[kind] ?? "unresolved-fail-closed",
    authority: provenEffectIds.has(effectId),
    note: provenEffectIds.has(effectId)
      ? "Operation lane is owned by the specialized current-build runtime and its conservation gates."
      : "No transfer is emitted until operation placement is exact.",
  };
}

function runtimeEvidenceForEffect(effectId) {
  if (runtimeEvidenceCache.has(effectId)) return runtimeEvidenceCache.get(effectId);
  const idPattern = new RegExp(`(?<!\\d)${effectId}(?!\\d)`);
  const sourceFiles = [...runtimeSourceText]
    .filter(([, text]) => idPattern.test(text))
    .map(([file]) => path.relative(ROOT, file).replaceAll("\\", "/"));
  const testFunctions = [];
  for (const [file, original] of runtimeSourceOriginal) {
    for (const chunk of original.split(/(?=#\[test\])/g)) {
      if (!chunk.startsWith("#[test]")) continue;
      if (!idPattern.test(chunk.replaceAll("_", ""))) continue;
      const name = chunk.match(/fn\s+([A-Za-z0-9_]+)/)?.[1];
      if (name) testFunctions.push({
        file: path.relative(ROOT, file).replaceAll("\\", "/"),
        function: name,
      });
    }
  }
  const runtimeConfigLocations = [];
  const runtimeReferenceLocations = [];
  walkRuntimeConfig(runtime, "$", effectId, runtimeConfigLocations, runtimeReferenceLocations);
  const evidence = {
    source_files_containing_exact_id: sourceFiles,
    test_functions: testFunctions,
    runtime_config_locations: uniqueStrings(runtimeConfigLocations),
    runtime_reference_locations: uniqueStrings(runtimeReferenceLocations),
  };
  runtimeEvidenceCache.set(effectId, evidence);
  return evidence;
}

function walkRuntimeConfig(value, pointer, effectId, directOutput, referenceOutput) {
  if (value === null || typeof value !== "object") return;
  if (!Array.isArray(value)) {
    const directEffect = Object.entries(value).some(([key, child]) =>
      /effect_id$/.test(key) && Number(child) === effectId);
    const effectArray = Object.entries(value).some(([key, child]) =>
      /effect_ids$/.test(key) && Array.isArray(child)
      && child.some((entry) => Number(entry) === effectId));
    if (directEffect) directOutput.push(pointer);
    if (effectArray) referenceOutput.push(pointer);
  }
  for (const [key, child] of Object.entries(value)) {
    walkRuntimeConfig(child, `${pointer}/${key}`, effectId, directOutput, referenceOutput);
  }
}

function descendantIdsForSkill(skill) {
  return uniqueSorted([
    ...(skill.passive_owner_buff_ids ?? []),
    ...(skill.owner_family_candidates ?? []).map((row) => row.buff_id),
    ...(skill.semantic_owner_candidates ?? []).map((row) => row.buff_id ?? row.effect_id ?? row.id),
    ...(skill.exact_relationship_candidates ?? []).flatMap((row) => [
      ...(row.runtime_buff_ids ?? []),
      ...(row.source_buff_ids ?? []),
      ...(row.historical_effects ?? []).map((effect) => effect.effect_id),
    ]),
  ]);
}

function discoveryEvidenceForEffect(effectId) {
  return aoyi.skills.flatMap((skill) => [
    ...(skill.owner_family_candidates ?? [])
      .filter((row) => Number(row.buff_id) === effectId)
      .map((row) => ({
        parent_skill_id: Number(skill.skill_id),
        source: "owner-family-candidate",
        match_strength: row.owner_match_strength,
        relationship: row.relationship,
        design_name: row.design_name ?? null,
        localized_name: row.name ?? null,
        localized_description: row.description ?? null,
        modifier_source_ids: row.modifier_source_ids ?? [],
        formula_statuses: row.formula_statuses ?? [],
        historical_effect: row.historical_effect ?? null,
        historical_relations: row.historical_relations ?? [],
      })),
    ...(skill.semantic_owner_candidates ?? [])
      .filter((row) => Number(row.buff_id ?? row.effect_id ?? row.id) === effectId)
      .map((row) => ({
        parent_skill_id: Number(skill.skill_id),
        source: "semantic-owner-candidate",
        relationship: row.relationship_source ?? null,
        recipient_scope: row.recipient_scope ?? null,
        rdps_disposition: row.rdps_disposition ?? null,
        proof_state: row.proof_state ?? null,
        transformed_attribute_id: row.transformed_attribute_id ?? null,
        matching_terms: row.matching_terms ?? [],
      })),
  ]);
}

function installedBuffEvidence(effectId) {
  const row = buffById.get(Number(effectId));
  if (!row) return null;
  return {
    id: Number(row.Id),
    level: row.Level ?? null,
    design_name: row.NameDesign ?? null,
    localized_name: row.Name ?? null,
    localized_description: row.Desc ?? null,
    duration_rules: row.DestroyParam ?? [],
    stacking_rule: row.RepeatAddRule ?? [],
    tags: row.Tags ?? [],
    special_attributes: row.SpecialAttr ?? [],
  };
}

function addDescendantLink(index, rawEffectId, parentId, source, exact) {
  const effectId = Number(rawEffectId);
  if (!Number.isSafeInteger(effectId) || effectId <= 0) return;
  const rows = index.get(effectId) ?? [];
  if (!rows.some((row) => row.parent_skill_id === parentId && row.source === source)) {
    rows.push({ parent_skill_id: parentId, source, exact });
  }
  index.set(effectId, rows);
}

function numericIds(rows) {
  return uniqueSorted((rows ?? []).map((row) =>
    Number(row?.buff_id ?? row?.effect_id ?? row?.target_id ?? row)));
}

function uniqueSorted(values) {
  return [...new Set(values.map(Number).filter((value) =>
    Number.isSafeInteger(value) && value > 0))].sort((left, right) => left - right);
}

function uniqueStrings(values) {
  return [...new Set(values
    .filter((value) => value !== null && value !== undefined && String(value).length > 0)
    .map(String))].sort((left, right) => left.localeCompare(right));
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8").replace(/^\uFEFF/, ""));
}

function assertIdentity(value, label) {
  if (String(value) !== BUILD) throw new Error(`${label} is build ${value}, expected ${BUILD}`);
}

function assertCount(actual, expected, label) {
  if (actual !== expected) throw new Error(`${label}: got ${actual}, expected ${expected}`);
}

function fileIdentity(file) {
  const bytes = fs.readFileSync(file);
  return {
    path: path.relative(ROOT, file).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function sha256(value) {
  return crypto.createHash("sha256").update(JSON.stringify(value)).digest("hex");
}
