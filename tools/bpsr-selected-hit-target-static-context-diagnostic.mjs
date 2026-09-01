#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const [groupArg, identityArg, monsterArg, entityAttributeArg, bulletArg, outputArg] =
  process.argv.slice(2);
if (!groupArg || !identityArg || !monsterArg || !entityAttributeArg || !bulletArg || !outputArg) {
  throw new Error(
    "Usage: node tools/bpsr-selected-hit-target-static-context-diagnostic.mjs " +
      "<group-relative.json> <target-identity.json> <MonsterTable.json> " +
      "<EntityAttributeTable.json> <BulletTable.json> <output.json>",
  );
}
const groupPath = path.resolve(groupArg);
const identityPath = path.resolve(identityArg);
const monsterPath = path.resolve(monsterArg);
const entityAttributePath = path.resolve(entityAttributeArg);
const bulletPath = path.resolve(bulletArg);
const outputPath = path.resolve(outputArg);
if (fs.existsSync(outputPath) || fs.existsSync(`${outputPath}.partial`)) {
  throw new Error(`Refusing to overwrite output or partial output: ${outputPath}`);
}

const group = readJson(groupPath);
const identity = readJson(identityPath);
const monsters = readJson(monsterPath);
const entityAttributes = readJson(entityAttributePath);
const bullets = readJson(bulletPath);
validateInputs(group, identity);
const entityAttributeIndex = entityAttributeRows(entityAttributes);

const identities = new Map(identity.observations.map((row) => [key(row), row]));
if (identities.size !== identity.observations.length) {
  throw new Error("target identity proof contains duplicate action keys");
}
const distinctStaticTargets = new Map();
const selectedRows = observations(group);
const rows = selectedRows.map((row) => {
  const target = identities.get(key(row));
  if (!target || Number(target.target_entity_uuid) !== Number(row.target_entity_uuid)) {
    throw new Error(`missing or mismatched target identity for ${key(row)}`);
  }
  const context = staticContext(target, monsters, entityAttributeIndex, bullets);
  distinctStaticTargets.set(context.identity_key, context.catalog_entry);
  return { ...row, target_identity: target, target_static_context: context.observation };
});
if (rows.length !== identity.observations.length) {
  throw new Error("group and target identity observation counts differ");
}

const hasContextDiagnostics = rows.every((row) =>
  Object.hasOwn(row, "base") && Object.hasOwn(row, "output"));
const conflicts = hasContextDiagnostics
  ? rows.filter((row) => row.baseline_context_conflicting)
  : [];
const baseline = hasContextDiagnostics
  ? summarizeContexts(rows, completeRetainedContextParts)
  : null;
const withTargetIdentity = hasContextDiagnostics
  ? summarizeContexts(rows, (row) => [
      ...completeRetainedContextParts(row),
      row.target_identity.actor_kind,
      row.target_identity.numeric_monster_id,
      row.target_identity.level,
      row.target_static_context.static_signature,
    ])
  : null;
const conflictWithTargetIdentity = hasContextDiagnostics
  ? summarizeContexts(conflicts, (row) => [
      ...completeRetainedContextParts(row),
      row.target_identity.actor_kind,
      row.target_identity.numeric_monster_id,
      row.target_identity.level,
      row.target_static_context.static_signature,
    ])
  : null;
const catalog = [...distinctStaticTargets.values()].sort((left, right) =>
  Number(left.numeric_id) - Number(right.numeric_id));

const report = {
  schema_version: 2,
  generated_by: "rlogs-bpsr-selected-hit-target-static-context-diagnostic",
  game_build: group.game_build,
  selection: group.selection,
  inputs: {
    selection_proof: receipt(groupPath),
    target_identity_proof: receipt(identityPath),
    monster_table: receipt(monsterPath),
    entity_attribute_table: receipt(entityAttributePath),
    bullet_table: receipt(bulletPath),
  },
  policy: {
    exact_numeric_ids_and_build_are_authoritative: true,
    target_actor_kind_and_identity_are_event_time_evidence: true,
    target_allegiance_assumed: false,
    localized_names_are_evidence_only: true,
    monster_table_attribute_id_is_an_exact_foreign_key: true,
    bullet_table_id_is_an_exact_foreign_key: true,
    absent_static_mitigation_fields_are_not_zero: true,
    fight_value_coefficient_is_not_assumed_to_be_defense_or_mitigation: true,
    current_character_or_target_snapshots_substituted: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  },
  summary: {
    selected_observations: rows.length,
    exact_numeric_target_identities: rows.filter((row) =>
      row.target_identity.exact_identity_kind === "numeric-monster-id").length,
    monster_target_observations: rows.filter((row) =>
      row.target_identity.actor_kind === "monster").length,
    projectile_target_observations: rows.filter((row) =>
      row.target_identity.actor_kind === "projectile").length,
    observations_with_level: rows.filter((row) => row.target_identity.level !== null).length,
    observations_with_exact_static_table_route: rows.filter((row) =>
      row.target_static_context.route_complete).length,
    observations_with_static_mitigation_scalar: rows.filter((row) =>
      row.target_static_context.static_mitigation_scalar_present).length,
    distinct_static_targets: catalog.length,
    distinct_monster_targets: catalog.filter((row) => row.actor_kind === "monster").length,
    distinct_projectile_targets: catalog.filter((row) => row.actor_kind === "projectile").length,
    context_diagnostics_available: hasContextDiagnostics,
    baseline_conflicting_contexts: baseline?.conflicting_repeated_context_count ?? null,
    baseline_conflicting_observations: baseline?.conflicting_repeated_observation_count ?? null,
  },
  target_catalog: catalog,
  diagnostics: {
    baseline,
    target_identity_level_and_static_signature: withTargetIdentity,
    original_conflict_target_identity_level_and_static_signature: conflictWithTargetIdentity,
  },
  observations: rows,
  conclusion: {
    every_selected_target_has_exact_numeric_identity: rows.every((row) =>
      row.target_identity.exact_identity_kind === "numeric-monster-id"),
    every_selected_target_has_exact_static_table_route: rows.every((row) =>
      row.target_static_context.route_complete),
    static_tables_supply_damage_mitigation_scalar: rows.some((row) =>
      row.target_static_context.static_mitigation_scalar_present),
    target_static_context_eliminates_all_original_conflicts: hasContextDiagnostics
      ? conflictWithTargetIdentity.conflicting_repeated_context_count === 0
      : null,
    target_static_context_reduces_original_conflicts: hasContextDiagnostics
      ? conflictWithTargetIdentity.conflicting_repeated_context_count <
        baseline.conflicting_repeated_context_count
      : null,
    exact_target_mitigation_formula_proven: false,
    exact_damage_formula_proven: false,
    provider_rdps_credit_allowed: false,
  },
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
const partialPath = `${outputPath}.partial`;
fs.writeFileSync(partialPath, `${JSON.stringify(report, null, 2)}\n`);
fs.renameSync(partialPath, outputPath);
console.log(JSON.stringify({ output: outputPath, summary: report.summary, conclusion: report.conclusion }, null, 2));

function staticContext(target, monsters, entityAttributeIndex, bullets) {
  const id = String(target.numeric_monster_id);
  if (target.actor_kind === "monster") {
    const monster = monsters[id] ?? null;
    const attributeId = integer(monster?.AttributeId);
    const attributes = attributeId === null
      ? null
      : entityAttributeIndex.get(String(attributeId)) ?? null;
    const routeComplete = Boolean(monster && attributes);
    const retained = attributes ? normalizedEntityAttributeRow(attributes) : null;
    const signature = JSON.stringify(["monster", id, attributeId, retained]);
    return {
      identity_key: `monster:${id}`,
      observation: {
        route: "MonsterTable.Id -> MonsterTable.AttributeId -> EntityAttributeTable.Id",
        route_complete: routeComplete,
        static_signature: signature,
        monster_attribute_id: attributeId,
        static_mitigation_scalar_present: false,
      },
      catalog_entry: {
        actor_kind: "monster",
        numeric_id: Number(id),
        localized_name_evidence: monster?.Name ?? null,
        monster_attribute_id: attributeId,
        route_complete: routeComplete,
        monster_row: monster,
        entity_attribute_formula_seed_row: retained,
        explicit_damage_mitigation_fields: [],
        static_mitigation_scalar_present: false,
        static_signature: signature,
      },
    };
  }
  if (target.actor_kind === "projectile") {
    const bullet = bullets[id] ?? null;
    const signature = JSON.stringify(["projectile", id, bullet]);
    return {
      identity_key: `projectile:${id}`,
      observation: {
        route: "BulletTable.Id",
        route_complete: Boolean(bullet),
        static_signature: signature,
        monster_attribute_id: null,
        static_mitigation_scalar_present: false,
      },
      catalog_entry: {
        actor_kind: "projectile",
        numeric_id: Number(id),
        localized_name_evidence: bullet?.Name ?? null,
        route_complete: Boolean(bullet),
        bullet_row: bullet,
        explicit_damage_mitigation_fields: [],
        static_mitigation_scalar_present: false,
        static_signature: signature,
      },
    };
  }
  const signature = JSON.stringify([target.actor_kind, id]);
  return {
    identity_key: `${target.actor_kind}:${id}`,
    observation: {
      route: "unresolved-actor-kind",
      route_complete: false,
      static_signature: signature,
      monster_attribute_id: null,
      static_mitigation_scalar_present: false,
    },
    catalog_entry: {
      actor_kind: target.actor_kind,
      numeric_id: Number(id),
      route_complete: false,
      explicit_damage_mitigation_fields: [],
      static_mitigation_scalar_present: false,
      static_signature: signature,
    },
  };
}

function key(row) {
  return `${row.session_id}:${row.sequence}`;
}

function completeRetainedContextParts(row) {
  return [
    row.base,
    Object.entries(row.raw_values_by_attribute_id ?? {})
      .sort(([a], [b]) => Number(a) - Number(b))
      .map(([id, value]) => [Number(id), Number(value)]),
    row.lifecycle?.source_config_id ?? "<null>",
    row.lifecycle?.status_state ?? "<null>",
    row.lifecycle?.status_stacks ?? "<null>",
    row.target_entity_uuid,
    row.source_attribute_state_id,
    row.target_attribute_state_id,
    row.source_status_state_id,
    row.target_status_state_id,
  ];
}

function summarizeContexts(rows, keyOf) {
  const contexts = new Map();
  for (const row of rows) {
    const contextKey = JSON.stringify(keyOf(row));
    const context = contexts.get(contextKey) ?? { observations: 0, outputs: new Set() };
    context.observations += 1;
    context.outputs.add(Number(row.output));
    contexts.set(contextKey, context);
  }
  let repeated = 0;
  let repeatedObservations = 0;
  let conflicting = 0;
  let conflictingObservations = 0;
  let maximumOutputs = 0;
  for (const context of contexts.values()) {
    maximumOutputs = Math.max(maximumOutputs, context.outputs.size);
    if (context.observations < 2) continue;
    repeated += 1;
    repeatedObservations += context.observations;
    if (context.outputs.size > 1) {
      conflicting += 1;
      conflictingObservations += context.observations;
    }
  }
  return {
    context_count: contexts.size,
    repeated_context_count: repeated,
    repeated_observation_count: repeatedObservations,
    conflicting_repeated_context_count: conflicting,
    conflicting_repeated_observation_count: conflictingObservations,
    maximum_distinct_outputs_in_one_context: maximumOutputs,
  };
}

function validateInputs(group, identity) {
  if (!Number.isSafeInteger(Number(group?.schema_version)) ||
      typeof group.game_build !== "string" || observations(group).length === 0) {
    throw new Error("expected an exact-build selection with observations");
  }
  if (group.policy?.damage_target_is_allegiance_neutral === false) {
    throw new Error("selection explicitly rejects allegiance-neutral damage targets");
  }
  if (![1, 2].includes(identity?.schema_version) ||
      identity.generated_by !== "rlogs-bpsr-selected-action-target-identity-proof" ||
      identity.game_build !== group.game_build ||
      identity.policy?.target_allegiance_assumed !== false ||
      !Array.isArray(identity.observations)) {
    throw new Error(`expected fail-closed build-${group.game_build} target identity proof`);
  }
}

function observations(value) {
  if (Array.isArray(value?.observations)) return value.observations;
  if (Array.isArray(value?.formula_surface?.groups)) {
    const byKey = new Map();
    for (const group of value.formula_surface.groups) {
      for (const row of group.examples ?? []) byKey.set(key(row), row);
    }
    return [...byKey.values()];
  }
  return [];
}

function entityAttributeRows(value) {
  if (Array.isArray(value?.rows)) {
    if (value.schema_version !== 2 ||
        value.generated_by !== "rlogs-bpsr-entity-attribute-table-proof" ||
        value.source?.expected_hash_matches !== true ||
        value.table?.primary_key_index_matches_rows !== true) {
      throw new Error("raw entity attribute proof is not exact-build schema-2 evidence");
    }
    return new Map(value.rows.map((row) => [String(row.id), row]));
  }
  return new Map(Object.entries(value ?? {}));
}

function normalizedEntityAttributeRow(row) {
  if (Object.hasOwn(row, "fight_value_coefficient")) {
    return {
      Id: integer(row.id),
      Level: poolArray(row.level),
      Season: poolArray(row.season),
      SeasonLv: poolArray(row.season_level),
      SeasonRank: poolArray(row.season_rank),
      IsLoadRank: row.is_load_rank ?? null,
      FightValueCoe: integer(row.fight_value_coefficient),
    };
  }
  return {
    Id: integer(row.Id),
    Level: arrayOrNull(row.Level),
    Season: arrayOrNull(row.Season),
    SeasonLv: arrayOrNull(row.SeasonLv),
    SeasonRank: arrayOrNull(row.SeasonRank),
    IsLoadRank: row.IsLoadRank ?? null,
    FightValueCoe: integer(row.FightValueCoe),
  };
}

function poolArray(value) {
  return arrayOrNull(value?.int_array);
}

function integer(value) {
  const number = Number(value);
  return Number.isSafeInteger(number) ? number : null;
}

function arrayOrNull(value) {
  return Array.isArray(value) ? value : null;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function receipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: filePath,
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex").toUpperCase(),
  };
}
