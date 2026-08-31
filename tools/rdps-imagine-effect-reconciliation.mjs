#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const gameBuild = process.argv[2] ?? "24687926";
const inventoryRoot = path.join(
  repoRoot,
  "plugins",
  "games",
  "blue-protocol-star-resonance",
  "research",
  "game-file-inventory",
  "global",
  `steam-${gameBuild}`,
);
const ledgerPath = process.argv[3]
  ? path.resolve(process.argv[3])
  : path.join(inventoryRoot, "current-aoyi-rdps-origin-ledger.candidate.json");
const reconciliationPath = process.argv[4]
  ? path.resolve(process.argv[4])
  : path.join(inventoryRoot, "rdps-observed-effect-reconciliation.v1.json");
const outputPath = process.argv[5]
  ? path.resolve(process.argv[5])
  : path.join(inventoryRoot, "rdps-imagine-effect-reconciliation.v1.json");
const priorLedgerPath = process.argv[6]
  ? path.resolve(process.argv[6])
  : path.join(path.dirname(inventoryRoot), "steam-24609362", "current-aoyi-rdps-origin-ledger.candidate.json");
const carryForwardPath = process.argv[7]
  ? path.resolve(process.argv[7])
  : path.join(inventoryRoot, "formula-proof-carry-forward.v2.json");
const recipientScopePath = process.argv[8]
  ? path.resolve(process.argv[8])
  : path.join(inventoryRoot, "rdps-recipient-scope-ledger.v2.json");
const historicalOriginsPath = process.argv[9]
  ? path.resolve(process.argv[9])
  : path.join(
      repoRoot,
      "plugins",
      "games",
      "blue-protocol-star-resonance",
      "research",
      "runtime-evidence",
      "global",
      "steam-24252055",
      "observed-status-origins.v1.json",
    );
const staticFormulaPath = process.argv[10]
  ? path.resolve(process.argv[10])
  : path.join(inventoryRoot, "static-formula-evidence.v1.json");
const runtimeProofPath = process.argv[11]
  ? path.resolve(process.argv[11])
  : path.join(inventoryRoot, "imagine-runtime-provider-recipient-proof.v1.json");
const componentFormulaProofPath = process.argv[12]
  ? path.resolve(process.argv[12])
  : path.join(inventoryRoot, "imagine-formula-proof.v1.json");

const ledger = readJson(ledgerPath);
const reconciliation = readJson(reconciliationPath);
const priorLedger = readJson(priorLedgerPath);
const carryForward = readJson(carryForwardPath);
const recipientScope = readJson(recipientScopePath);
const historicalOrigins = readJson(historicalOriginsPath);
const staticFormula = readJson(staticFormulaPath);
const runtimeProof = readJson(runtimeProofPath);
const componentFormulaProof = readJson(componentFormulaProofPath);

if (String(ledger.game_build) !== String(gameBuild)) {
  throw new Error(`Imagine ledger build ${ledger.game_build} does not match ${gameBuild}`);
}
if (String(reconciliation.game_build) !== String(gameBuild)) {
  throw new Error(
    `Observed-effect reconciliation build ${reconciliation.game_build} does not match ${gameBuild}`,
  );
}
if (String(runtimeProof.game_build) !== String(gameBuild)) {
  throw new Error(
    `Imagine runtime proof build ${runtimeProof.game_build} does not match ${gameBuild}`,
  );
}
if (String(componentFormulaProof.game_build) !== String(gameBuild)) {
  throw new Error(
    `Imagine component-formula proof build ${componentFormulaProof.game_build} does not match ${gameBuild}`,
  );
}

const observedEffects = new Map(
  reconciliation.effects.map((effect) => [Number(effect.effect_id), effect]),
);
const priorComponents = new Map(
  priorLedger.skills.flatMap((skill) =>
    (skill.component_routes ?? []).map((component) => [
      componentKey(skill.skill_id, component.component_id),
      component,
    ]),
  ),
);
const carryForwardProofs = new Map(
  carryForward.proofs.map((proof) => [Number(proof.effect_id), proof]),
);
const historicalEffects = new Map(
  historicalOrigins.effects.map((effect) => [Number(effect.effect_id), effect]),
);
const historicalRelationsBySourceConfig = groupBy(
  historicalOrigins.relations,
  (relation) => Number(relation.source_config_id),
);
const scopeCandidates = recipientScope.candidates;
const staticFormulaSources = staticFormula.sources;
const runtimeSkills = new Map(
  runtimeProof.skills.map((skill) => [Number(skill.imagine_skill_id), skill]),
);
const runtimeComponents = new Map(
  runtimeProof.skills.flatMap((skill) =>
    (skill.components ?? []).map((component) => [
      componentKey(skill.imagine_skill_id, component.component_id),
      component,
    ]),
  ),
);
const currentComponentFormulaProofs = new Map(
  componentFormulaProof.components.map((component) => [
    componentKey(component.imagine_skill_id, component.component_id),
    component,
  ]),
);

// These joins are intentionally component-specific. A single Imagine action can
// expose several independent mechanics through one SkillEffect row (for example,
// a party shield plus a recipient-triggered attack). Matching the entire tier
// row by skill or effect id would let a defensive scalar masquerade as an
// offensive formula. Keep each semantic field attached only to the component it
// actually describes.
const structuredTierSemanticRoutes = new Map([
  ["blade-sweep-target-armor-reduction", ["Block DMG Reduction Bonus", "Armor Penetration"]],
  ["time-decree-external-cooldown-speed", ["CD decrease"]],
  ["thunder-roar-electro-shield", ["Shield"]],
  ["fatal-spiral-shared-all-element-bonus", ["All-Element Bonus"]],
  ["superconductor-surge-mechanical-power-main-stats", ["Main Attribute Enhanced"]],
  [
    "superconductor-surge-mechanical-power-healing-received",
    ["Healing Received up"],
  ],
  [
    "functional-amp-external-attack",
    ["ATK bonus", "Attack SPD Boost", "Casting SPD Boost"],
  ],
  [
    "celestial-guardian-morale-reduction",
    [
      "Celestial Spirit Guard ATK Reduction",
      "Celestial Spirit Guard Vulnerability",
      "Celestial Spirit Guard Elemental Resistance Reduction",
    ],
  ],
  [
    "celestial-guardian-party-shield",
    ["[Celestial Spirit Guard]<br>Celestial Spirit Guard Shield"],
  ],
]);

const evidence = {
  observedEffects,
  priorComponents,
  carryForwardProofs,
  historicalEffects,
  historicalRelationsBySourceConfig,
  scopeCandidates,
  staticFormulaSources,
  runtimeSkills,
  runtimeComponents,
  currentComponentFormulaProofs,
};
const skills = ledger.skills.map((skill) => reconcileSkill(skill, evidence));
const components = skills.flatMap((skill) => skill.components);
const transferableComponents = components.filter(
  (component) => component.attribution_lane !== "owner_only",
);
const offensiveTransferComponents = components.filter(
  (component) => component.attribution_lane === "offensive_rdps_candidate",
);
const stableRoutes = components.filter(
  (component) => component.current_static_route.state === "stable_across_builds",
);
const historicallyObservedExternalRoutes = offensiveTransferComponents.filter(
  (component) => component.historical_packet_evidence.external_lifecycle_observed,
);
const historicalReplayCandidates = offensiveTransferComponents.filter(
  (component) => component.readiness.historical_replay_candidate,
);
const currentPromotionEligible = offensiveTransferComponents.filter(
  (component) => component.readiness.current_build_promotion_eligible,
);

const report = {
  schema_version: 1,
  game: "blue-protocol-star-resonance",
  game_build: String(gameBuild),
  generated_by: "tools/rdps-imagine-effect-reconciliation.mjs",
  policy: {
    preserve_every_imagine: true,
    preserve_every_component: true,
    description_is_identity_hint_not_runtime_authority: true,
    transfer_requires_exact_component_route_and_packet_provider_recipient_proof: true,
    ordinary_owner_damage_is_never_support_credit: true,
    defensive_and_healing_lanes_never_invent_damage_credit: true,
    ambiguous_or_unobserved_routes_remain_visible: true,
    historical_packet_evidence_is_never_relabeled_as_current_build_observation: true,
    stable_static_identity_may_carry_proof_context_but_not_runtime_enablement: true,
    matching_build_lifecycle_and_conservation_replay_are_required_for_current_promotion: true,
    run_owned_equipped_identity_and_tier_are_authoritative_for_that_run: true,
    later_profile_snapshots_never_rewrite_historical_imagine_identity_or_tier: true,
    runtime_damage_identity_is_evidence_not_automatic_support_credit: true,
    ambiguous_canonical_damage_collisions_remain_unallocated: true,
  },
  inputs: {
    imagine_ledger: relative(ledgerPath),
    observed_effect_reconciliation: relative(reconciliationPath),
    prior_build_imagine_ledger: relative(priorLedgerPath),
    formula_proof_carry_forward: relative(carryForwardPath),
    recipient_scope_ledger: relative(recipientScopePath),
    historical_status_origins: relative(historicalOriginsPath),
    static_formula_evidence: relative(staticFormulaPath),
    imagine_runtime_provider_recipient_proof: relative(runtimeProofPath),
    imagine_current_build_component_formula_proof: relative(componentFormulaProofPath),
  },
  summary: {
    imagine_skills: skills.length,
    component_routes: components.length,
    exact_damage_chain_ids: ledger.summary.exact_damage_chain_ids,
    missing_exact_damage_chain_ids: ledger.summary.missing_exact_damage_chain_ids,
    exact_damage_attr_rows: ledger.summary.exact_damage_attr_rows,
    missing_exact_damage_attr_rows: ledger.summary.missing_exact_damage_attr_rows,
    transferable_component_routes: transferableComponents.length,
    offensive_rdps_candidate_routes: offensiveTransferComponents.length,
    defensive_or_healing_routes: components.filter(
      (component) => component.attribution_lane === "defense_or_healing",
    ).length,
    owner_only_routes: components.filter(
      (component) => component.attribution_lane === "owner_only",
    ).length,
    routing_only_routes: components.filter(
      (component) => component.attribution_lane === "routing_only",
    ).length,
    components_with_direct_observed_effect: components.filter(
      (component) => component.packet_evidence.direct_observed_effect_ids.length > 0,
    ).length,
    components_with_configured_packet_origin: components.filter(
      (component) => component.packet_evidence.configured_origin_effect_ids.length > 0,
    ).length,
    components_with_any_packet_evidence: components.filter(
      (component) => component.packet_evidence.state !== "not_observed_in_current_packet_corpus",
    ).length,
    components_with_current_structured_tier_parameters: components.filter(
      (component) =>
        component.formula_evidence.current_structured_tier_parameter_proofs.length > 0,
    ).length,
    offensive_routes_with_current_structured_tier_parameters:
      offensiveTransferComponents.filter(
        (component) =>
          component.formula_evidence.current_structured_tier_parameter_proofs.length > 0,
      ).length,
    components_with_external_player_windows: components.filter(
      (component) => component.packet_evidence.resolved_external_player_to_player_windows > 0,
    ).length,
    components_with_matching_build_external_lifecycle: components.filter(
      (component) => component.readiness.external_provider_recipient_window_observed,
    ).length,
    matching_build_skills_with_equipped_provider_observations:
      runtimeProof.summary.skills_with_equipped_provider_observations,
    matching_build_equipped_provider_observations:
      runtimeProof.summary.equipped_provider_observations,
    matching_build_components_with_status_lifecycle:
      runtimeProof.summary.components_with_status_lifecycle_observations,
    matching_build_components_with_external_player_lifecycle:
      runtimeProof.summary.components_with_external_player_lifecycle_rows,
    matching_build_components_with_exact_damage:
      runtimeProof.summary.components_with_exact_damage_observations,
    matching_build_components_with_exact_formula_scalar:
      componentFormulaProof.summary.components_with_exact_current_scalar,
    matching_build_formula_components_with_external_lifecycle:
      componentFormulaProof.summary.components_with_matching_build_external_lifecycle,
    matching_build_formula_components_runtime_enabled:
      componentFormulaProof.summary.offensive_components_runtime_enabled,
    matching_build_unallocated_ambiguous_damage_rows: runtimeProof.skills.reduce(
      (total, skill) =>
        total + Number(skill.summary?.unallocated_ambiguous_damage_rows ?? 0),
      0,
    ),
    matching_build_unallocated_ambiguous_observed_damage: runtimeProof.skills
      .reduce(
        (total, skill) =>
          total + BigInt(skill.summary?.unallocated_ambiguous_observed_damage ?? "0"),
        0n,
      )
      .toString(),
    stable_component_routes_across_builds: stableRoutes.length,
    offensive_routes_with_historical_external_lifecycle:
      historicallyObservedExternalRoutes.length,
    offensive_routes_with_guarded_historical_replay_candidate:
      historicalReplayCandidates.length,
    offensive_routes_current_build_promotion_eligible: currentPromotionEligible.length,
    offensive_routes_ready_for_counterfactual_replay: offensiveTransferComponents.filter(
      (component) => component.readiness.state === "ready_for_counterfactual_replay",
    ).length,
    offensive_routes_requiring_more_proof: offensiveTransferComponents.filter(
      (component) => component.readiness.state !== "ready_for_counterfactual_replay",
    ).length,
  },
  invariants: buildInvariants({
    ledger,
    skills,
    components,
    offensiveTransferComponents,
    currentPromotionEligible,
  }),
  skills,
};

fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({ output: relative(outputPath), summary: report.summary }, null, 2));

function reconcileSkill(skill, evidence) {
  const {
    observedEffects: observedById,
    priorComponents: priorByKey,
    carryForwardProofs: carryForwardById,
    historicalEffects: historicalById,
    historicalRelationsBySourceConfig: historicalRelationsBySource,
    scopeCandidates: scopeRows,
    staticFormulaSources: formulaRows,
    runtimeSkills: runtimeSkillsById,
    runtimeComponents: runtimeComponentsByKey,
    currentComponentFormulaProofs: currentComponentFormulaProofsByKey,
  } = evidence;
  const runtimeSkill = runtimeSkillsById.get(Number(skill.skill_id));
  const components = (skill.component_routes ?? []).map((component) => {
    const effectIds = uniqueNumbers(component.effect_ids ?? []);
    const sourceConfigIds = uniqueNumbers(component.source_config_ids ?? []);
    const directObserved = effectIds
      .map((effectId) => observedById.get(effectId))
      .filter(Boolean);
    const configuredOrigins = [];
    for (const effect of observedById.values()) {
      const matchingOrigins = (effect.packet_origins ?? []).filter((origin) =>
        sourceConfigIds.includes(Number(origin.source_config_id)) ||
        effectIds.includes(Number(origin.source_config_id)),
      );
      if (matchingOrigins.length > 0) {
        configuredOrigins.push({
          observed_effect_id: Number(effect.effect_id),
          origins: matchingOrigins,
          transfer_proof: effect.transfer_proof,
          packet_lifecycle: effect.packet_lifecycle,
          formula_endpoint: effect.formula_endpoint,
        });
      }
    }

    const externalWindows = [
      ...directObserved.map((effect) => effect.packet_lifecycle),
      ...configuredOrigins.map((entry) => entry.packet_lifecycle),
    ].reduce(
      (total, lifecycle) =>
        total + Number(lifecycle?.resolved_external_player_to_player_windows ?? 0),
      0,
    );
    const runtimeComponent = runtimeComponentsByKey.get(
      componentKey(skill.skill_id, component.component_id),
    );
    const currentComponentFormulaProof = currentComponentFormulaProofsByKey.get(
      componentKey(skill.skill_id, component.component_id),
    );
    const runtimeExternalPlayerStatusRows = Number(
      runtimeComponent?.summary?.external_player_status_rows ?? 0,
    );
    const matchingBuildExternalLifecycleObserved =
      externalWindows > 0 || runtimeExternalPlayerStatusRows > 0;
    const attributionLane = attributionLaneFor(component);
    const priorComponent = priorByKey.get(componentKey(skill.skill_id, component.component_id));
    const currentStaticRoute = compareStaticRoute(component, priorComponent);
    const relevantEffectIds = uniqueNumbers([
      ...effectIds,
      ...configuredOrigins.map((entry) => entry.observed_effect_id),
    ]);
    const historicalEffectRows = relevantEffectIds
      .map((effectId) => historicalById.get(effectId))
      .filter(Boolean);
    const historicalRelations = uniqueObjects(
      [...sourceConfigIds, ...effectIds].flatMap(
        (sourceConfigId) => historicalRelationsBySource.get(sourceConfigId) ?? [],
      ),
    );
    const matchingScopeRows = scopeRows.filter((candidate) =>
      candidateMatchesComponent(candidate, relevantEffectIds, component),
    );
    const scopeHistoricalRows = matchingScopeRows
      .map((candidate) => candidate.historical_packet_evidence)
      .filter(Boolean);
    const historicalPacketEvidence = compactHistoricalEvidence(
      historicalEffectRows,
      historicalRelations,
      scopeHistoricalRows,
    );
    const matchingFormulaRows = formulaRows.filter((source) =>
      formulaSourceMatchesComponent(source, relevantEffectIds, sourceConfigIds),
    );
    const carryProofs = relevantEffectIds
      .map((effectId) => carryForwardById.get(effectId))
      .filter(Boolean);
    const structuredTierProofs = componentStructuredTierProofs(skill, component);
    const formulaEvidence = compactFormulaEvidence(
      matchingFormulaRows,
      carryProofs,
      structuredTierProofs,
      currentComponentFormulaProof,
    );
    const packetState =
      directObserved.length > 0 && configuredOrigins.length > 0
        ? "direct_effect_and_configured_origin_observed"
        : directObserved.length > 0
          ? "direct_effect_observed"
          : configuredOrigins.length > 0
            ? "configured_origin_observed"
            : "not_observed_in_current_packet_corpus";
    const packetCurrentExactFormula = [
      ...directObserved,
      ...configuredOrigins.map((entry) => entry),
    ]
      .some((entry) => entry?.transfer_proof?.exact_scalar_available === true);
    const proofCurrentExactFormula =
      currentComponentFormulaProof?.exact_component_scalar_available === true;
    const currentExactFormula = packetCurrentExactFormula || proofCurrentExactFormula;
    const currentBuildConservationReplayComplete =
      currentComponentFormulaProof?.attribution_contract
        ?.current_build_conservation_replay_complete === true ||
      directObserved.some(
        (entry) => entry?.transfer_proof?.current_build_conservation_replay_complete === true,
      );
    const guardedHistoricalFormula = formulaEvidence.exact_historical_formula_available;
    const historicalReplayCandidate =
      attributionLane === "offensive_rdps_candidate" &&
      currentStaticRoute.state === "stable_across_builds" &&
      historicalPacketEvidence.external_lifecycle_observed &&
      guardedHistoricalFormula;
    const currentBuildPromotionEligible =
      attributionLane === "offensive_rdps_candidate" &&
      currentStaticRoute.state === "stable_across_builds" &&
      matchingBuildExternalLifecycleObserved &&
      currentExactFormula &&
      currentBuildConservationReplayComplete;
    const blockers = readinessBlockers({
      attributionLane,
      currentStaticRoute,
      relevantEffectIds,
      historicalPacketEvidence,
      formulaEvidence,
      matchingBuildExternalLifecycleObserved,
      currentExactFormula,
      currentBuildConservationReplayComplete,
      currentBuildPromotionEligible,
    });

    return {
      imagine_skill_id: Number(skill.skill_id),
      imagine_name: skill.name,
      component_id: component.component_id,
      role: component.role,
      effect_ids: effectIds,
      source_config_ids: sourceConfigIds,
      recipient_scope: component.recipient_scope,
      rdps_disposition: component.rdps_disposition,
      static_proof_state: component.proof_state,
      attribution_lane: attributionLane,
      current_static_route: currentStaticRoute,
      packet_evidence: {
        state: packetState,
        direct_observed_effect_ids: directObserved.map((effect) => Number(effect.effect_id)),
        configured_origin_effect_ids: uniqueNumbers(
          configuredOrigins.map((entry) => entry.observed_effect_id),
        ),
        resolved_external_player_to_player_windows: externalWindows,
        direct_observed_effects: directObserved.map(compactObservedEffect),
        configured_origins: configuredOrigins,
      },
      matching_build_runtime_evidence: compactRuntimeComponentEvidence(runtimeComponent),
      historical_packet_evidence: historicalPacketEvidence,
      formula_evidence: formulaEvidence,
      matching_recipient_scope_rules: matchingScopeRows.map(compactScopeCandidate),
      readiness: {
        state: currentBuildPromotionEligible
          ? "ready_for_counterfactual_replay"
          : attributionLane !== "offensive_rdps_candidate"
            ? "not_an_offensive_rdps_lane"
            : currentStaticRoute.state === "stable_across_builds" &&
                matchingBuildExternalLifecycleObserved &&
                currentExactFormula
              ? "requires_counterfactual_replay_and_conservation"
            : historicalReplayCandidate
              ? "guarded_historical_replay_candidate_requires_current_build_confirmation"
              : "requires_formula_or_provider_recipient_proof",
        historical_replay_candidate: historicalReplayCandidate,
        current_build_promotion_eligible: currentBuildPromotionEligible,
        exact_scalar_available_in_current_packet_reconciliation: packetCurrentExactFormula,
        exact_scalar_available_in_current_build_component_formula_proof:
          proofCurrentExactFormula,
        exact_current_build_scalar_available: currentExactFormula,
        current_build_conservation_replay_complete: currentBuildConservationReplayComplete,
        exact_historical_formula_available: guardedHistoricalFormula,
        external_provider_recipient_window_observed:
          matchingBuildExternalLifecycleObserved,
        historical_external_provider_recipient_window_observed:
          historicalPacketEvidence.external_lifecycle_observed,
        blockers,
      },
    };
  });

  return {
    skill_id: Number(skill.skill_id),
    item_id: skill.item_id,
    monster_id: skill.monster_id,
    season_id: skill.season_id,
    rarity_type: skill.rarity_type,
    classification: skill.classification,
    name: skill.name,
    monster_name: skill.monster_name,
    recipient_description_hint: skill.recipient_evidence,
    candidate_classes_from_description: skill.candidate_classes,
    direct_attribute_transformation_evidence: skill.direct_attribute_transformation_evidence,
    exact_damage_chain_candidates: skill.exact_damage_chain_candidates,
    exact_relationship_candidates: skill.exact_relationship_candidates,
    matching_build_runtime_identity: compactRuntimeSkillIdentity(runtimeSkill),
    components,
  };
}

function compareStaticRoute(current, prior) {
  if (!prior) {
    return { state: "new_in_current_build", prior_route_present: false };
  }
  const currentSignature = staticRouteSignature(current);
  const priorSignature = staticRouteSignature(prior);
  return {
    state:
      currentSignature === priorSignature ? "stable_across_builds" : "changed_across_builds",
    prior_route_present: true,
    prior_build: "24609362",
    current_build: String(gameBuild),
    changed_fields:
      currentSignature === priorSignature ? [] : changedRouteFields(current, prior),
  };
}

function staticRouteSignature(component) {
  return JSON.stringify({
    component_id: component.component_id ?? null,
    role: component.role ?? null,
    effect_ids: uniqueNumbers(component.effect_ids ?? []),
    source_config_ids: uniqueNumbers(component.source_config_ids ?? []),
    recipient_scope: component.recipient_scope ?? null,
    rdps_disposition: component.rdps_disposition ?? null,
  });
}

function changedRouteFields(current, prior) {
  const fields = [
    "component_id",
    "role",
    "effect_ids",
    "source_config_ids",
    "recipient_scope",
    "rdps_disposition",
  ];
  return fields.filter((field) => {
    const left = field.endsWith("_ids")
      ? JSON.stringify(uniqueNumbers(current[field] ?? []))
      : JSON.stringify(current[field] ?? null);
    const right = field.endsWith("_ids")
      ? JSON.stringify(uniqueNumbers(prior[field] ?? []))
      : JSON.stringify(prior[field] ?? null);
    return left !== right;
  });
}

function compactHistoricalEvidence(effectRows, relations, scopeRows) {
  const statusEvents = sum(effectRows, "status_events");
  const openedWindows = sum(effectRows, "window_count");
  const crossActorWindows = sum(effectRows, "cross_actor_window_count");
  const playerRecipientWindows = sum(effectRows, "target_player_window_count");
  const monsterRecipientWindows = sum(effectRows, "target_monster_window_count");
  const scopeOpenedWindows = sum(scopeRows, "opened_windows");
  const scopeCrossActorWindows = sum(scopeRows, "cross_actor_windows");
  const scopeOwnerLinkedProviderWindows = sum(scopeRows, "owner_linked_player_provider_windows");
  const authoritativeOpenedWindows = Math.max(openedWindows, scopeOpenedWindows);
  const authoritativeCrossActorWindows = Math.max(
    crossActorWindows,
    scopeCrossActorWindows,
  );
  return {
    authority: "historical-build-packet-corpus-research-only",
    packet_build: String(historicalOrigins.game_build),
    effect_ids: uniqueNumbers(effectRows.map((row) => row.effect_id)),
    status_events: statusEvents,
    opened_windows: authoritativeOpenedWindows,
    cross_actor_windows: authoritativeCrossActorWindows,
    player_recipient_windows: Math.max(
      playerRecipientWindows,
      sum(scopeRows, "player_recipient_windows"),
    ),
    monster_recipient_windows: Math.max(
      monsterRecipientWindows,
      sum(scopeRows, "monster_recipient_windows"),
    ),
    owner_linked_player_provider_windows: scopeOwnerLinkedProviderWindows,
    packet_origin_observations:
      sum(effectRows, "packet_origin_observations") + sum(relations, "observation_count"),
    external_lifecycle_observed: authoritativeCrossActorWindows > 0,
    current_build_promotion_eligible: false,
    effect_rows: effectRows,
    source_relations: relations,
  };
}

function compactFormulaEvidence(
  formulaRows,
  carryProofs,
  structuredTierProofs,
  currentComponentFormulaProof,
) {
  const resolvedCurrentStaticRows = formulaRows.filter(
    (row) => row.formula_magnitude_resolved === true && row.static_gate_resolved === true,
  );
  const exactCarryProofs = carryProofs.filter(
    (proof) =>
      (proof.proven_coefficients ?? []).length > 0 &&
      typeof proof.exact_damage_counterfactual === "string" &&
      proof.exact_damage_counterfactual.length > 0,
  );
  return {
    current_static_formula_rows: formulaRows.map(compactFormulaSource),
    current_static_formula_resolved: resolvedCurrentStaticRows.length > 0,
    current_structured_tier_parameter_proofs: structuredTierProofs,
    current_structured_tier_parameters_resolved: structuredTierProofs.length > 0,
    structured_tier_parameters_are_not_runtime_authority: true,
    current_build_component_formula_proof: currentComponentFormulaProof
      ? compactCurrentComponentFormulaProof(currentComponentFormulaProof)
      : null,
    historical_carry_forward_proofs: carryProofs,
    exact_historical_formula_available: exactCarryProofs.length > 0,
    current_build_runtime_enabled: carryProofs.some(
      (proof) => proof.current_build_runtime_enabled === true,
    ),
  };
}

function compactCurrentComponentFormulaProof(proof) {
  return {
    imagine_skill_id: Number(proof.imagine_skill_id),
    component_id: proof.component_id,
    effect_ids: uniqueNumbers(proof.effect_ids ?? []),
    proof_state: proof.proof_state,
    exact_component_scalar_available: proof.exact_component_scalar_available === true,
    exact_native_equation_available: proof.exact_native_equation_available === true,
    matching_build_external_lifecycle_observed:
      proof.matching_build_external_lifecycle_observed === true,
    tier_values: proof.tier_values,
    tier_parameter_pairs: proof.tier_parameter_pairs,
    localized_tier_values_percent: proof.localized_tier_values_percent,
    duration_millis: proof.duration_millis,
    lockout_effect_id: proof.lockout_effect_id,
    lockout_duration_millis: proof.lockout_duration_millis,
    equation: proof.equation,
    interpretation: proof.interpretation,
    current_class_primary_transforms: proof.current_class_primary_transforms,
    current_primary_transform: proof.current_primary_transform,
    historical_transition_guard: proof.historical_transition_guard,
    attribution_contract: proof.attribution_contract,
    current_runtime_summary: proof.current_runtime_summary,
    remaining_proof_obligations: proof.remaining_proof_obligations,
  };
}

function componentStructuredTierProofs(skill, component) {
  const allowedLabels = structuredTierSemanticRoutes.get(component.component_id);
  if (!allowedLabels) return [];
  const allowed = new Set(allowedLabels);
  return (skill.active_modifier_parameter_evidence ?? [])
    .map((proof) => {
      const tiers = (proof.tiers ?? [])
        .map((tier) => ({
          tier: tier.tier,
          fields: (tier.fields ?? []).filter((field) => allowed.has(field.semantic_role)),
        }))
        .filter((tier) => tier.fields.length > 0);
      const semanticLabels = (proof.semantic_labels ?? []).filter((label) => allowed.has(label));
      if (semanticLabels.length === 0 || tiers.length === 0) return null;
      return {
        skill_effect_id: proof.skill_effect_id,
        active_effect_ids: proof.active_effect_ids,
        semantic_labels: semanticLabels,
        parameter_encoding: proof.parameter_encoding,
        raw_units_per_percent: proof.raw_units_per_percent,
        raw_units_per_decimal: proof.raw_units_per_decimal,
        duration_seconds: proof.duration_seconds,
        tiers,
        proof_state: proof.proof_state,
        runtime_authority: proof.runtime_authority,
        component_join_proof:
          "explicit current-build component-to-SkillEffect semantic-field route",
      };
    })
    .filter(Boolean);
}

function compactFormulaSource(source) {
  return {
    source_rule_id: source.source_rule_id,
    source_id: source.source_id,
    source_name: source.source_name,
    effect_ids: source.effect_ids,
    classification: source.classification,
    formula_magnitude_resolved: source.formula_magnitude_resolved,
    static_gate_resolved: source.static_gate_resolved,
    runtime_selector_required: source.runtime_selector_required,
    remaining_static_blockers: source.remaining_static_blockers,
    remaining_runtime_requirements: source.remaining_runtime_requirements,
    components: source.components,
  };
}

function compactScopeCandidate(candidate) {
  return {
    source_rule_id: candidate.source_rule_id,
    source_id: candidate.source_id,
    source_name: candidate.source_name,
    contribution_mode: candidate.contribution_mode,
    scope_resolution: candidate.scope_resolution,
    scope_queue: candidate.scope_queue,
    transfer_gate: candidate.transfer_gate,
    current_build_promotion_eligible: candidate.current_build_promotion_eligible,
    remaining_requirement: candidate.remaining_requirement,
  };
}

function readinessBlockers({
  attributionLane,
  currentStaticRoute,
  relevantEffectIds,
  historicalPacketEvidence,
  formulaEvidence,
  matchingBuildExternalLifecycleObserved,
  currentExactFormula,
  currentBuildConservationReplayComplete,
  currentBuildPromotionEligible,
}) {
  if (attributionLane !== "offensive_rdps_candidate") return [];
  if (currentBuildPromotionEligible) return [];
  const blockers = [];
  if (currentStaticRoute.state !== "stable_across_builds") {
    blockers.push("static component route is new or changed in the current build");
  }
  if (relevantEffectIds.length === 0) {
    blockers.push("exact packet effect identity is unresolved");
  }
  if (!currentExactFormula && !formulaEvidence.exact_historical_formula_available) {
    blockers.push("exact conserved damage counterfactual is not proven");
  }
  if (
    !matchingBuildExternalLifecycleObserved &&
    !historicalPacketEvidence.external_lifecycle_observed
  ) {
    blockers.push("no historical cross-actor provider/recipient lifecycle is proven");
  }
  if (!matchingBuildExternalLifecycleObserved) {
    blockers.push("matching-build external provider/recipient lifecycle is not observed");
  }
  if (!currentExactFormula) {
    blockers.push("matching-build exact component scalar is unresolved");
  }
  if (!currentBuildConservationReplayComplete) {
    blockers.push("matching-build damage replay and party conservation are incomplete");
  }
  return blockers;
}

function candidateMatchesComponent(candidate, effectIds, component) {
  const candidateEffectIds = uniqueNumbers([
    ...(candidate.effect_ids ?? []),
    ...(candidate.declared_effect_ids ?? []),
    ...(candidate.runtime_related_effect_ids ?? []),
  ]);
  if (candidateEffectIds.some((effectId) => effectIds.includes(effectId))) return true;
  return (candidate.current_component_evidence ?? []).some(
    (entry) => entry.component_id === component.component_id,
  );
}

function formulaSourceMatchesComponent(source, effectIds, sourceConfigIds) {
  const formulaEffectIds = uniqueNumbers(source.effect_ids ?? []);
  if (formulaEffectIds.some((effectId) => effectIds.includes(effectId))) {
    return true;
  }
  // A numeric source id is not globally unique across the decoded tables. For
  // example, Blade Sweep recount parent 270 and season-rogue entry 270 are
  // unrelated routes. Once a formula row declares an effect family, that
  // effect identity is authoritative and a same-number source fallback would
  // create a false formula join. Keep the fallback only for formula evidence
  // that has no effect identity to join through.
  if (formulaEffectIds.length > 0) return false;
  const numericSource = Number(String(source.source_id ?? "").split(":").at(-1));
  return Number.isFinite(numericSource) && sourceConfigIds.includes(numericSource);
}

function buildInvariants({
  ledger: currentLedger,
  skills: reconciledSkills,
  components: reconciledComponents,
  offensiveTransferComponents: offensiveComponents,
  currentPromotionEligible: promotedComponents,
}) {
  const laneTotal = ["offensive_rdps_candidate", "defense_or_healing", "owner_only", "routing_only"]
    .map((lane) => reconciledComponents.filter((component) => component.attribution_lane === lane).length)
    .reduce((total, count) => total + count, 0);
  const invalidPromotions = promotedComponents.filter(
    (component) =>
      component.current_static_route.state !== "stable_across_builds" ||
      !component.readiness.exact_current_build_scalar_available ||
      !component.readiness.external_provider_recipient_window_observed ||
      !component.readiness.current_build_conservation_replay_complete,
  );
  const invalidEffectBearingFormulaJoins = reconciledComponents.flatMap((component) =>
    (component.formula_evidence?.current_static_formula_rows ?? [])
      .filter((source) => {
        const formulaEffectIds = uniqueNumbers(source.effect_ids ?? []);
        return (
          formulaEffectIds.length > 0 &&
          !formulaEffectIds.some((effectId) => component.effect_ids.includes(effectId))
        );
      })
      .map((source) => ({
        component_id: component.component_id,
        component_effect_ids: component.effect_ids,
        source_rule_id: source.source_rule_id,
        formula_effect_ids: source.effect_ids,
      })),
  );
  const invalidStructuredTierJoins = reconciledComponents.flatMap((component) => {
    const allowed = new Set(structuredTierSemanticRoutes.get(component.component_id) ?? []);
    return (component.formula_evidence?.current_structured_tier_parameter_proofs ?? [])
      .flatMap((proof) => proof.semantic_labels ?? [])
      .filter((label) => !allowed.has(label))
      .map((label) => ({ component_id: component.component_id, semantic_label: label }));
  });
  const thunderstrike = reconciledComponents.find(
    (component) => component.component_id === "thunder-roar-recipient-thunderstrike",
  );
  const checks = {
    all_imagine_skills_preserved: reconciledSkills.length === currentLedger.skills.length,
    all_component_routes_classified: laneTotal === reconciledComponents.length,
    exact_damage_chain_inventory_complete:
      Number(currentLedger.summary.missing_exact_damage_chain_ids) === 0,
    exact_damage_attribute_inventory_complete:
      Number(currentLedger.summary.missing_exact_damage_attr_rows) === 0,
    no_current_promotion_without_stable_route_formula_and_lifecycle:
      invalidPromotions.length === 0,
    current_promotion_count_not_greater_than_offensive_routes:
      promotedComponents.length <= offensiveComponents.length,
    effect_bearing_formula_joins_share_exact_effect_identity:
      invalidEffectBearingFormulaJoins.length === 0,
    structured_tier_parameters_follow_explicit_component_semantics:
      invalidStructuredTierJoins.length === 0,
    thunderstrike_never_inherits_shield_tier_parameters:
      (thunderstrike?.formula_evidence?.current_structured_tier_parameter_proofs ?? []).length ===
      0,
  };
  return {
    all_pass: Object.values(checks).every(Boolean),
    checks,
    invalid_current_promotion_component_ids: invalidPromotions.map(
      (component) => component.component_id,
    ),
    invalid_effect_bearing_formula_joins: invalidEffectBearingFormulaJoins,
    invalid_structured_tier_joins: invalidStructuredTierJoins,
  };
}

function attributionLaneFor(component) {
  const role = String(component.role ?? "").toLowerCase();
  const disposition = String(component.rdps_disposition ?? "").toLowerCase();
  const scope = String(component.recipient_scope ?? "").toLowerCase();
  if (
    disposition.includes("defense-lane") ||
    disposition.includes("healing-attribution") ||
    role.includes("shield") ||
    role.includes("healing")
  ) {
    return "defense_or_healing";
  }
  if (
    disposition.includes("uptime-only") ||
    disposition.includes("routing") ||
    role.includes("emitter-and-status-origin") ||
    role.includes("transformation-and-output-origin")
  ) {
    return "routing_only";
  }
  if (
    role.includes("transferable-external") ||
    role.includes("recipient-triggered-produced-damage") ||
    disposition.includes("counterfactual") ||
    disposition.includes("credit-only-other-players") ||
    (scope.includes("allies") && !disposition.includes("never")) ||
    (scope.includes("enemy") && !disposition.includes("never"))
  ) {
    return "offensive_rdps_candidate";
  }
  return "owner_only";
}

function compactObservedEffect(effect) {
  return {
    effect_id: Number(effect.effect_id),
    display_name: effect.display_name,
    proof_queue: effect.proof_queue,
    endpoint_resolution: effect.endpoint_resolution,
    source_resolution: effect.source_resolution,
    transfer_proof: effect.transfer_proof,
    packet_lifecycle: effect.packet_lifecycle,
    packet_origins: effect.packet_origins,
    formula_endpoint: effect.formula_endpoint,
  };
}

function compactRuntimeSkillIdentity(skill) {
  if (!skill) {
    return {
      state: "not_observed_equipped_in_matching_build_history",
      summary: {
        equipped_provider_observations: 0,
        unique_equipped_providers: 0,
        observed_tiers: [],
        observed_scenes: [],
        unallocated_ambiguous_damage_rows: 0,
        unallocated_ambiguous_observed_damage: "0",
      },
      provider_observations: [],
    };
  }
  return {
    state:
      Number(skill.summary?.equipped_provider_observations ?? 0) > 0
        ? "run_owned_equipped_identity_and_tier_observed"
        : "not_observed_equipped_in_matching_build_history",
    summary: skill.summary,
    provider_observations: skill.provider_observations ?? [],
    unallocated_damage_observations: skill.unallocated_damage_observations ?? [],
  };
}

function compactRuntimeComponentEvidence(component) {
  if (!component) {
    return {
      state: "component_not_present_in_matching_build_runtime_proof",
      summary: {
        status_rows: 0,
        external_player_status_rows: 0,
        damage_rows: 0,
        observed_damage: "0",
        influence_rows: 0,
      },
    };
  }
  const summary = component.summary ?? {};
  const hasEvidence =
    Number(summary.status_rows ?? 0) > 0 ||
    Number(summary.damage_rows ?? 0) > 0 ||
    Number(summary.influence_rows ?? 0) > 0;
  return {
    state: hasEvidence
      ? "matching_build_runtime_evidence_observed"
      : "not_observed_in_matching_build_history",
    role: component.role,
    effect_ids: component.effect_ids ?? [],
    exact_damage_ids: component.exact_damage_ids ?? [],
    canonical_damage_ability_ids: component.canonical_damage_ability_ids ?? [],
    recipient_scope: component.recipient_scope,
    rdps_disposition: component.rdps_disposition,
    proof_state: component.proof_state,
    summary,
  };
}

function componentKey(skillId, componentId) {
  return `${Number(skillId)}:${String(componentId)}`;
}

function groupBy(values, keyForValue) {
  const groups = new Map();
  for (const value of values ?? []) {
    const key = keyForValue(value);
    const group = groups.get(key);
    if (group) {
      group.push(value);
    } else {
      groups.set(key, [value]);
    }
  }
  return groups;
}

function uniqueObjects(values) {
  const seen = new Set();
  return (values ?? []).filter((value) => {
    const identity = JSON.stringify(value);
    if (seen.has(identity)) return false;
    seen.add(identity);
    return true;
  });
}

function sum(values, field) {
  return (values ?? []).reduce(
    (total, value) => total + Number(value?.[field] ?? 0),
    0,
  );
}

function uniqueNumbers(values) {
  return [...new Set(values.map(Number).filter(Number.isFinite))].sort((a, b) => a - b);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function relative(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}
