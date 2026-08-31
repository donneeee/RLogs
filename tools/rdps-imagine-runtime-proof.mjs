#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const gameBuild = process.argv[2] ?? "24687926";
const buildRoot = path.join(
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
  : path.join(buildRoot, "current-aoyi-rdps-origin-ledger.candidate.json");
const historyRoot = process.argv[4]
  ? path.resolve(process.argv[4])
  : path.join(repoRoot, "runtime-data", "history", "combat-meter");
const outputPath = process.argv[5]
  ? path.resolve(process.argv[5])
  : path.join(buildRoot, "imagine-runtime-provider-recipient-proof.v1.json");

const ledger = readJson(ledgerPath);
if (String(ledger.game_build) !== String(gameBuild)) {
  throw new Error(`Imagine ledger build ${ledger.game_build} does not match ${gameBuild}`);
}

const skillProofs = new Map(
  ledger.skills.map((skill) => [Number(skill.skill_id), createSkillProof(skill)]),
);
const files = fs.existsSync(historyRoot)
  ? fs
      .readdirSync(historyRoot, { withFileTypes: true })
      .filter((entry) => entry.isFile() && entry.name.endsWith(".combat-history.v1.json"))
      .map((entry) => path.join(historyRoot, entry.name))
      .sort()
  : [];

const corpus = {
  history_files_discovered: files.length,
  history_files_parsed: 0,
  matching_build_files: 0,
  nonmatching_build_files: 0,
  invalid_history_files: [],
  matching_build_empty_files: 0,
  matching_build_runs: 0,
  matching_build_runs_without_broad_view: 0,
};

for (const filePath of files) {
  let history;
  try {
    history = readJson(filePath);
    corpus.history_files_parsed += 1;
  } catch (error) {
    corpus.invalid_history_files.push({
      file: relative(filePath),
      error: error instanceof Error ? error.message : String(error),
    });
    continue;
  }
  if (String(history.client_build) !== String(gameBuild)) {
    corpus.nonmatching_build_files += 1;
    continue;
  }
  corpus.matching_build_files += 1;
  const runs = history.runs ?? [];
  if (runs.length === 0) corpus.matching_build_empty_files += 1;

  for (const run of runs) {
    corpus.matching_build_runs += 1;
    const view = (run.views ?? []).find((candidate) =>
      candidate.id === "all" || candidate.kind === "all",
    );
    if (!view) {
      corpus.matching_build_runs_without_broad_view += 1;
      continue;
    }
    indexRun({ filePath, history, run, view, skillProofs });
  }
}

const skills = [...skillProofs.values()]
  .map(finalizeSkillProof)
  .sort((left, right) => left.imagine_skill_id - right.imagine_skill_id);
const components = skills.flatMap((skill) => skill.components);
const report = {
  schema_version: 1,
  game: "blue-protocol-star-resonance",
  game_build: String(gameBuild),
  generated_by: "tools/rdps-imagine-runtime-proof.mjs",
  policy: {
    equipped_ability_and_tier_are_runtime_identity_authority: true,
    latest_profile_snapshot_must_not_retroactively_override_a_run: true,
    broad_entire_run_view_is_indexed_once_to_avoid_segment_double_counting: true,
    status_routes_require_exact_component_effect_id: true,
    damage_routes_require_exact_current_ledger_damage_id: true,
    provider_identity_requires_equipped_imagine_ability_in_that_run: true,
    external_transfer_requires_distinct_source_and_player_target: true,
    unobserved_skills_and_components_remain_visible: true,
    packet_evidence_is_preserved_without_formula_or_recount_guesses: true,
  },
  inputs: {
    imagine_ledger: relative(ledgerPath),
    combat_history_root: relative(historyRoot),
  },
  corpus,
  summary: {
    imagine_skills: skills.length,
    component_routes: components.length,
    skills_with_equipped_provider_observations: count(
      skills,
      (skill) => skill.summary.equipped_provider_observations > 0,
    ),
    components_with_status_lifecycle_observations: count(
      components,
      (component) => component.summary.status_rows > 0,
    ),
    components_with_external_player_lifecycle_rows: count(
      components,
      (component) => component.summary.external_player_status_rows > 0,
    ),
    components_with_exact_damage_observations: count(
      components,
      (component) => component.summary.damage_rows > 0,
    ),
    components_with_exact_influence_observations: count(
      components,
      (component) => component.summary.influence_rows > 0,
    ),
    equipped_provider_observations: sum(
      skills,
      (skill) => skill.summary.equipped_provider_observations,
    ),
    status_rows: sum(components, (component) => component.summary.status_rows),
    external_player_status_rows: sum(
      components,
      (component) => component.summary.external_player_status_rows,
    ),
    status_applications: sum(
      components,
      (component) => component.summary.lifecycle.applied,
    ),
    status_removals: sum(
      components,
      (component) => component.summary.lifecycle.removed,
    ),
    damage_rows: sum(components, (component) => component.summary.damage_rows),
    observed_damage: String(
      components.reduce(
        (total, component) => total + BigInt(component.summary.observed_damage),
        0n,
      ),
    ),
    influence_rows: sum(components, (component) => component.summary.influence_rows),
    components_without_current_runtime_evidence: count(
      components,
      (component) =>
        component.summary.status_rows === 0 &&
        component.summary.damage_rows === 0 &&
        component.summary.influence_rows === 0,
    ),
  },
  skills,
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({ output: relative(outputPath), corpus, summary: report.summary }, null, 2));

function createSkillProof(skill) {
  const components = new Map(
    (skill.component_routes ?? []).map((component) => [
      component.component_id,
      {
        component_id: component.component_id,
        role: component.role,
        effect_ids: uniqueNumbers(component.effect_ids ?? []),
        source_config_ids: uniqueNumbers(component.source_config_ids ?? []),
        recipient_scope: component.recipient_scope,
        rdps_disposition: component.rdps_disposition,
        proof_state: component.proof_state,
        ...exactDamageRoute(skill, component),
        status_observations: [],
        damage_observations: [],
        influence_observations: [],
      },
    ]),
  );
  return {
    imagine_skill_id: Number(skill.skill_id),
    item_id: numberOrNull(skill.item_id),
    imagine_name: skill.name,
    provider_observations: [],
    unallocated_damage_observations: [],
    components,
  };
}

function exactDamageRoute(skill, component) {
  const explicit = uniqueNumbers([
    ...(component.effect_ids ?? []),
    ...(component.source_config_ids ?? []),
  ]);
  const candidates = (skill.exact_damage_chain_candidates ?? []).filter((candidate) => {
    const candidateIds = uniqueNumbers([
      candidate.skill_effect_id,
      ...(candidate.damage_ids ?? []),
      ...(candidate.resolved_damage_ids ?? []),
      ...(candidate.source_target_damage_ids ?? []),
      ...(candidate.exact_effect_source_ids ?? []),
    ]);
    return candidateIds.some((id) => explicit.includes(id));
  });
  const role = String(component.role ?? "");
  const disposition = String(component.rdps_disposition ?? "");
  const isDamageRoute = isProducedDamageRoute(role, disposition);
  const isRoutingOnly =
    role.includes("zero-formula") ||
    disposition.includes("routing-only") ||
    disposition.includes("never-count-as-damage");
  if (candidates.length === 0 && isDamageRoute) {
    candidates.push(...(skill.exact_damage_chain_candidates ?? []));
  }
  const candidateDamageIds = uniqueNumbers(
    candidates.flatMap((candidate) => [
      ...(candidate.damage_ids ?? []),
      ...(candidate.resolved_damage_ids ?? []),
      ...(candidate.source_target_damage_ids ?? []),
    ]),
  );
  const explicitDamageIds = explicit.filter((id) => candidateDamageIds.includes(id));
  const exactDamageIds = isRoutingOnly || !isDamageRoute
    ? []
    : explicitDamageIds.length > 0
      ? explicitDamageIds
      : isDamageRoute
        ? candidateDamageIds
        : [];
  const canonicalDamageAbilityIds = uniqueNumbers(
    candidates.flatMap((candidate) => [
      ...(candidate.damage_attr_rows ?? [])
        .filter((row) => exactDamageIds.includes(Number(row.Id)))
        .map((row) => row.TypeEnum),
      ...(candidate.source_target_damage_attr_rows ?? [])
        .filter((row) => exactDamageIds.includes(Number(row.Id)))
        .map((row) => row.TypeEnum),
    ]),
  );
  return {
    exact_damage_ids: exactDamageIds,
    canonical_damage_ability_ids: canonicalDamageAbilityIds,
  };
}

function isProducedDamageRoute(role, disposition) {
  if (
    disposition.includes("never-count-as-damage") ||
    role.startsWith("external-produced-healing")
  ) {
    return false;
  }
  return (
    (role.includes("owner-produced") && role.includes("damage")) ||
    role.includes("recipient-triggered-produced-damage") ||
    (role.includes("directly-referenced") && role.includes("damage")) ||
    role.includes("summon-damage")
  );
}

function indexRun({ filePath, history, run, view, skillProofs: proofs }) {
  const actors = view.actors ?? [];
  const actorsById = new Map(actors.map((actor) => [String(actor.actor_id), actor]));
  for (const provider of actors) {
    const equipped = [
      ...(provider.primary_loadout ?? []).map((slot) => ({ source: "primary", ...slot })),
      ...(provider.auxiliary_loadout ?? []).map((slot) => ({ source: "auxiliary", ...slot })),
    ];
    for (const slot of equipped) {
      const skillProof = proofs.get(Number(slot.ability_id));
      if (!skillProof) continue;
      const observation = providerObservation({
        filePath,
        history,
        run,
        provider,
        slot,
      });
      skillProof.provider_observations.push(observation);

      for (const component of skillProof.components.values()) {
        const effectIds = new Set(component.effect_ids.map(String));
        for (const effect of provider.effects ?? []) {
          if (!effectIds.has(String(effect.effect_id))) continue;
          const target = actorsById.get(String(effect.target_actor_id));
          component.status_observations.push({
            ...observationIdentity(observation),
            effect_id: String(effect.effect_id),
            target_actor_id: stringOrNull(effect.target_actor_id),
            target_entity_uuid: stringOrNull(effect.target_entity_uuid),
            target_character_id: stringOrNull(target?.character_id),
            target_name: actorName(target),
            target_kind: stringOrNull(target?.actor_kind),
            target_scope: targetScope(provider, target, effect),
            lifecycle: compactLifecycle(effect),
          });
        }

        for (const influence of view.damage_influences ?? []) {
          if (
            String(influence.provider_actor_id) !== String(provider.actor_id) ||
            !effectIds.has(String(influence.effect_id))
          ) {
            continue;
          }
          component.influence_observations.push({
            ...observationIdentity(observation),
            effect_id: String(influence.effect_id),
            recipient_actor_id: stringOrNull(influence.recipient_actor_id),
            recipient_entity_uuid: stringOrNull(influence.recipient_entity_uuid),
            affected_ability_id: stringOrNull(influence.affected_ability_id),
            target_actor_id: stringOrNull(influence.target_actor_id),
            target_entity_uuid: stringOrNull(influence.target_entity_uuid),
            first_observed_micros: integerString(influence.first_observed_micros),
            last_observed_micros: integerString(influence.last_observed_micros),
            damage_event_count: number(influence.damage_event_count),
            observed_damage: integerString(influence.observed_damage),
            exact_integer_delta: integerString(influence.exact_integer_delta),
            exact_rational_deltas: influence.exact_rational_deltas ?? [],
            damage_context_complete: influence.damage_context_complete === true,
          });
        }
      }

      const canonicalRoutes = new Map();
      for (const component of skillProof.components.values()) {
        for (const abilityId of component.canonical_damage_ability_ids) {
          const routes = canonicalRoutes.get(String(abilityId)) ?? [];
          routes.push(component);
          canonicalRoutes.set(String(abilityId), routes);
        }
      }
      for (const ability of provider.abilities ?? []) {
        const routes = canonicalRoutes.get(String(ability.ability_id)) ?? [];
        if (routes.length === 0) continue;
        const damageObservation = {
          ...observationIdentity(observation),
          canonical_ability_id: String(ability.ability_id),
          casts: number(ability.casts),
          hits: number(ability.hits),
          critical_hits: number(ability.critical_hits),
          damage: integerString(ability.damage),
          effective_damage: integerString(ability.effective_damage),
          targets: (ability.targets ?? []).map((targetRow) => ({
            actor_id: stringOrNull(targetRow.actor_id),
            entity_uuid: stringOrNull(targetRow.entity_uuid),
            damage: integerString(targetRow.damage),
            effective_damage: integerString(targetRow.effective_damage),
            hits: number(targetRow.hits),
            critical_hits: number(targetRow.critical_hits),
          })),
        };
        if (routes.length === 1) {
          routes[0].damage_observations.push({
            ...damageObservation,
            allocation_state: "exact-single-component-route",
            exact_damage_ids: routes[0].exact_damage_ids,
          });
        } else {
          skillProof.unallocated_damage_observations.push({
            ...damageObservation,
            allocation_state: "ambiguous-canonical-id-shared-by-components",
            candidate_components: routes.map((route) => ({
              component_id: route.component_id,
              exact_damage_ids: route.exact_damage_ids,
            })),
          });
        }
      }
    }
  }
}

function providerObservation({ filePath, history, run, provider, slot }) {
  return {
    history_file: relative(filePath),
    session_id: stringOrNull(history.session_id),
    run_index: number(run.run_index),
    scene_id: numberOrNull(run.scene_id),
    activity_id: numberOrNull(run.activity_id),
    activity_family_id: numberOrNull(run.activity_family_id),
    terminal_state: stringOrNull(run.terminal_state),
    provider_actor_id: String(provider.actor_id),
    provider_entity_uuid: stringOrNull(provider.entity_uuid),
    provider_character_id: stringOrNull(provider.character_id),
    provider_name: actorName(provider),
    provider_kind: stringOrNull(provider.actor_kind),
    equipped_source: slot.source,
    equipped_slot_id: numberOrNull(slot.slot_id),
    equipped_item_id: numberOrNull(slot.item_id),
    equipped_tier: numberOrNull(slot.tier),
  };
}

function observationIdentity(observation) {
  return {
    history_file: observation.history_file,
    session_id: observation.session_id,
    run_index: observation.run_index,
    scene_id: observation.scene_id,
    terminal_state: observation.terminal_state,
    provider_actor_id: observation.provider_actor_id,
    provider_entity_uuid: observation.provider_entity_uuid,
    provider_character_id: observation.provider_character_id,
    provider_name: observation.provider_name,
    equipped_source: observation.equipped_source,
    equipped_slot_id: observation.equipped_slot_id,
    equipped_item_id: observation.equipped_item_id,
    equipped_tier: observation.equipped_tier,
  };
}

function targetScope(provider, target, effect) {
  if (
    String(effect.target_actor_id) === String(provider.actor_id) ||
    (effect.target_entity_uuid != null &&
      String(effect.target_entity_uuid) === String(provider.entity_uuid))
  ) {
    return "provider_self";
  }
  if (!target) return "unresolved_target";
  if (target.actor_kind === "player") return "external_player";
  if (String(target.actor_kind ?? "").includes("party")) return "external_party_npc";
  if (target.actor_kind) return `external_${target.actor_kind}`;
  return "external_unknown_kind";
}

function finalizeSkillProof(skill) {
  const providerObservations = uniqueObjects(skill.provider_observations);
  const unallocatedDamage = uniqueObjects(skill.unallocated_damage_observations);
  const components = [...skill.components.values()].map((component) => {
    const status = uniqueObjects(component.status_observations);
    const damage = uniqueObjects(component.damage_observations);
    const influences = uniqueObjects(component.influence_observations);
    const lifecycle = status.reduce(
      (totals, row) => {
        for (const key of Object.keys(totals)) totals[key] += number(row.lifecycle[key]);
        return totals;
      },
      { applied: 0, refreshed: 0, stacked: 0, consumed: 0, removed: 0 },
    );
    return {
      component_id: component.component_id,
      role: component.role,
      effect_ids: component.effect_ids,
      source_config_ids: component.source_config_ids,
      exact_damage_ids: component.exact_damage_ids,
      canonical_damage_ability_ids: component.canonical_damage_ability_ids,
      recipient_scope: component.recipient_scope,
      rdps_disposition: component.rdps_disposition,
      proof_state: component.proof_state,
      summary: {
        status_rows: status.length,
        self_status_rows: count(status, (row) => row.target_scope === "provider_self"),
        external_player_status_rows: count(
          status,
          (row) => row.target_scope === "external_player",
        ),
        external_party_npc_status_rows: count(
          status,
          (row) => row.target_scope === "external_party_npc",
        ),
        other_or_unresolved_status_rows: count(
          status,
          (row) =>
            !["provider_self", "external_player", "external_party_npc"].includes(
              row.target_scope,
            ),
        ),
        unique_providers: new Set(status.map((row) => providerKey(row))).size,
        unique_external_player_recipients: new Set(
          status
            .filter((row) => row.target_scope === "external_player")
            .map((row) => row.target_character_id ?? row.target_entity_uuid ?? row.target_actor_id),
        ).size,
        lifecycle,
        damage_rows: damage.length,
        observed_damage: String(
          damage.reduce((total, row) => total + BigInt(row.damage), 0n),
        ),
        influence_rows: influences.length,
        influence_damage_events: sum(influences, (row) => row.damage_event_count),
        influence_observed_damage: String(
          influences.reduce((total, row) => total + BigInt(row.observed_damage), 0n),
        ),
        complete_damage_context_rows: count(
          influences,
          (row) => row.damage_context_complete,
        ),
      },
      status_observations: status,
      damage_observations: damage,
      influence_observations: influences,
    };
  });
  return {
    imagine_skill_id: skill.imagine_skill_id,
    item_id: skill.item_id,
    imagine_name: skill.imagine_name,
    summary: {
      equipped_provider_observations: providerObservations.length,
      unique_equipped_providers: new Set(providerObservations.map(providerKey)).size,
      observed_tiers: uniqueNumbers(
        providerObservations.map((observation) => observation.equipped_tier),
      ),
      observed_scenes: uniqueNumbers(
        providerObservations.map((observation) => observation.scene_id),
      ),
      unallocated_ambiguous_damage_rows: unallocatedDamage.length,
      unallocated_ambiguous_observed_damage: String(
        unallocatedDamage.reduce((total, row) => total + BigInt(row.damage), 0n),
      ),
    },
    provider_observations: providerObservations,
    unallocated_damage_observations: unallocatedDamage,
    components,
  };
}

function providerKey(row) {
  return row.provider_character_id ?? row.provider_entity_uuid ?? row.provider_actor_id;
}

function compactLifecycle(effect) {
  return {
    applied: number(effect.applied),
    refreshed: number(effect.refreshed),
    stacked: number(effect.stacked),
    consumed: number(effect.consumed),
    removed: number(effect.removed),
  };
}

function actorName(actor) {
  if (!actor) return null;
  return actor.display_name ?? actor.presentation_name ?? null;
}

function uniqueObjects(values) {
  const seen = new Set();
  const result = [];
  for (const value of values) {
    const key = JSON.stringify(value);
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(value);
  }
  return result;
}

function uniqueNumbers(values) {
  return [...new Set(values.map(numberOrNull).filter((value) => value != null))].sort(
    (left, right) => left - right,
  );
}

function integerString(value) {
  if (value == null || value === "") return "0";
  try {
    return String(BigInt(String(value)));
  } catch {
    return String(Math.trunc(Number(value) || 0));
  }
}

function stringOrNull(value) {
  return value == null ? null : String(value);
}

function number(value) {
  return Number(value ?? 0) || 0;
}

function numberOrNull(value) {
  if (value == null || value === "") return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function count(values, predicate) {
  return values.reduce((total, value) => total + (predicate(value) ? 1 : 0), 0);
}

function sum(values, selector) {
  return values.reduce((total, value) => total + Number(selector(value) ?? 0), 0);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function relative(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}
