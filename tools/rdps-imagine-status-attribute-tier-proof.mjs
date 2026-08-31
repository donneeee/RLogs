#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const [command, ...args] = process.argv.slice(2);
if (command === "build") {
  build(parseArguments(args));
} else if (command === "verify") {
  const options = parseArguments(args);
  verify(readJson(required(options, "input")));
} else {
  usage();
  process.exitCode = 2;
}

function build(options) {
  const gameBuild = required(options, "build");
  const fightLevelPath = path.resolve(required(options, "skill-fight-level-table"));
  const aoyiStarPath = path.resolve(required(options, "skill-aoyi-star-table"));
  const statusPath = path.resolve(required(options, "status-attribute-proof"));
  const ownershipPath = path.resolve(required(options, "provider-ownership-proof"));
  const outputPath = path.resolve(required(options, "output"));
  const fightLevelTable = readJson(fightLevelPath);
  const aoyiStarTable = readJson(aoyiStarPath);
  const status = readJson(statusPath);
  const ownership = readJson(ownershipPath);

  requireExact(String(status.expected_game_build) === gameBuild, "status proof build");
  requireExact(String(ownership.game_build) === gameBuild, "ownership build");
  requireExact(status.schema_version === 29, "status proof schema");
  requireExact(status.generated_by === "rlogs-bpsr-rdps-status-attribute-proof", "status proof generator");
  requireExact(status.policy?.runtime_use === "offline_research_only_not_loaded_by_live_parser", "status proof authority");
  requireExact(status.policy?.formula_inference === false, "status proof inference policy");
  requireExact(status.policy?.unresolved_evidence_is_hidden === false, "status proof unresolved policy");

  const fightLevel = fightLevelTable["397101"];
  requireExact(Number(fightLevel?.SkillId) === 3971, "Imagine skill identity");
  requireExact(Number(fightLevel?.SkillEffectId) === 397101, "Imagine skill-effect route");
  const basePair = [
    numberParameter(fightLevel?.FloatParameter, "attrA"),
    numberParameter(fightLevel?.FloatParameter, "attrB"),
  ];
  const tierRows = Object.values(aoyiStarTable)
    .filter((row) => Number(row.SkillId) === 3971)
    .sort((left, right) => Number(left.Level) - Number(right.Level));
  requireExact(
    JSON.stringify(tierRows.map((row) => Number(row.Level))) === JSON.stringify([1, 2, 3, 4, 5]),
    "current-build tier coverage",
  );
  const tierPairs = {
    0: basePair,
    ...Object.fromEntries(
      tierRows.map((row) => [String(Number(row.Level)), (row.BuffPar?.[0] ?? []).map(Number)]),
    ),
  };
  requireExact(
    JSON.stringify(tierPairs) === JSON.stringify({
      0: [750, 1000],
      1: [780, 1040],
      2: [960, 1280],
      3: [1140, 1520],
      4: [1320, 1760],
      5: [1500, 2000],
    }),
    "current-build loadout tier map",
  );

  const mainOccurrences = exactSingleEffectOccurrences(status, 11034);
  const healingOccurrences = exactSingleEffectOccurrences(status, 11802);
  const mainByKey = uniqueOccurrenceIndex(mainOccurrences, "main-stat raw-percent");
  const healingByKey = uniqueOccurrenceIndex(healingOccurrences, "healing-received add");
  const ownershipIndex = buildOwnershipIndex(ownership);
  const resolved = [];
  const unmatched = [];
  for (const [key, main] of mainByKey) {
    const heal = healingByKey.get(key);
    if (!heal) {
      unmatched.push({ key, missing_attribute_id: 11802 });
      continue;
    }
    requireExact(main.source_entity_uuid === heal.source_entity_uuid, `${key} provider agreement`);
    requireExact(main.instance_id === heal.instance_id, `${key} instance agreement`);
    requireExact(main.state === heal.state, `${key} lifecycle-state agreement`);
    const pair = [main.normalized_coefficient, heal.normalized_coefficient];
    const tiers = Object.entries(tierPairs)
      .filter(([, candidate]) => JSON.stringify(candidate) === JSON.stringify(pair))
      .map(([tier]) => Number(tier));
    if (tiers.length !== 1) {
      unmatched.push({ key, observed_pair: pair, candidate_tiers: tiers });
      continue;
    }
    const ownershipKey = [main.session_id, main.run_ordinal, main.source_entity_uuid].join("|");
    const owner = ownershipIndex.get(ownershipKey);
    requireExact(Boolean(owner), `${key} exact player ownership`);
    resolved.push({
      session_id: main.session_id,
      run_ordinal: main.run_ordinal,
      target_entity_uuid: main.target_entity_uuid,
      wire_capture_sequence: main.wire_capture_sequence,
      wire_observed_micros: main.wire_observed_micros,
      effect_id: 2110140,
      status_instance_id: main.instance_id,
      lifecycle_state: main.state,
      provider_entity_uuid: main.source_entity_uuid,
      provider_character_id: owner.character_id,
      provider_class_id: owner.class_id,
      provider_specialization_id: owner.specialization_id,
      loadout_tier: tiers[0],
      exact_attribute_pair: {
        main_stat_raw_percent_attribute_id: 11034,
        main_stat_raw_percent_units: pair[0],
        healing_received_add_attribute_id: 11802,
        healing_received_add_units: pair[1],
      },
      resolution_scope:
        "this exact provider/status-instance/recipient lifecycle occurrence only",
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    });
  }
  for (const [key] of healingByKey) {
    if (!mainByKey.has(key)) unmatched.push({ key, missing_attribute_id: 11034 });
  }
  resolved.sort(compareOccurrences);
  const providerGroups = groupProviderEvidence(resolved);
  const applied = ownership.resolutions
    .filter((row) => Number(row.effect_id) === 2110140)
    .reduce((sum, row) => sum + Number(row.status_state_counts?.applied ?? 0), 0);
  const removed = ownership.resolutions
    .filter((row) => Number(row.effect_id) === 2110140)
    .reduce((sum, row) => sum + Number(row.status_state_counts?.removed ?? 0), 0);
  requireExact(applied === 136 && removed === 136, "effect 2110140 lifecycle cohort");

  const report = {
    schema_version: 1,
    generated_by: "tools/rdps-imagine-status-attribute-tier-proof.mjs",
    game_build: gameBuild,
    effect_id: 2110140,
    imagine_skill_id: 3971,
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      remote_cast_packet_required: false,
      missing_remote_cast_is_synthesized: false,
      tier_resolution_is_occurrence_scoped: true,
      provider_tier_is_not_propagated_across_time_or_recipients: true,
      unresolved_lifecycles_are_retained: true,
      healing_received_is_never_damage_rdps: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      current_skill_fight_level_table: receipt(fightLevelPath),
      current_skill_aoyi_star_table: receipt(aoyiStarPath),
      status_attribute_proof: receipt(statusPath),
      provider_ownership_proof: receipt(ownershipPath),
    },
    exact_current_build_loadout_tier_parameter_pairs: tierPairs,
    attribute_contract: {
      main_stat_raw_percent_attribute_id: 11034,
      healing_received_add_attribute_id: 11802,
      join:
        "same session, run, target, wire, effect 2110140 status instance, provider, and lifecycle state",
      isolation:
        "each retained equation contains exactly one effect term and its raw delta divides exactly by signed presence delta",
    },
    summary: {
      selected_status_events: Number(
        status.effects?.find((effect) => Number(effect.effect_id) === 2110140)
          ?.selected_status_events ?? 0,
      ),
      applied_status_instances: applied,
      removed_status_instances: removed,
      exact_paired_attribute_occurrences: resolved.length,
      exact_base_tier_occurrences: resolved.filter((row) => row.loadout_tier === 0).length,
      exact_tier_5_occurrences: resolved.filter((row) => row.loadout_tier === 5).length,
      unresolved_applied_status_instances: applied - resolved.length,
      unmatched_clean_attribute_occurrences: unmatched.length,
      provider_groups: providerGroups.length,
      observed_damage_reassigned_to_provider: 0,
    },
    provider_tier_evidence: providerGroups,
    resolved_lifecycle_occurrences: resolved,
    unmatched_clean_attribute_occurrences: unmatched,
    remaining_proof_obligations: [
      "resolve tier independently for every lifecycle occurrence used by a damage counterfactual",
      "join the exact recipient class-selected primary and attack state at the lifecycle boundary",
      "select only recipient damage actions inside the proven lifecycle window",
      "prove current-build damage-stage operation order and integer rounding",
      "prove recipient debit equals provider credit without changing ordinary damage totals",
    ],
  };
  verify(report);
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(
    `wrote ${outputPath}: ${resolved.length} exact occurrence-scoped tiers, ${applied - resolved.length} unresolved applications`,
  );
}

function exactSingleEffectOccurrences(status, attributeId) {
  const system = status.wire_additive_equation_systems?.find(
    (entry) => Number(entry.attribute_id) === attributeId,
  );
  requireExact(Boolean(system), `attribute ${attributeId} equation system`);
  const occurrences = [];
  for (const equation of system.equations ?? []) {
    if (equation.terms?.length !== 1 || Number(equation.terms[0].effect_id) !== 2110140) continue;
    const signed = Number(equation.terms[0].signed_presence_delta);
    const raw = Number(equation.raw_attribute_delta);
    if (![-1, 1].includes(signed) || !Number.isSafeInteger(raw) || raw % signed !== 0) continue;
    for (const example of equation.examples ?? []) {
      const rows = (example.status_instances ?? []).filter(
        (row) => Number(row.effect_id) === 2110140,
      );
      requireExact(rows.length === 1, `attribute ${attributeId} exact status row`);
      requireExact(Number(equation.count) === (equation.examples ?? []).length, `attribute ${attributeId} complete example retention`);
      occurrences.push({
        session_id: String(example.session_id),
        run_ordinal: Number(example.run_ordinal),
        target_entity_uuid: Number(example.target_entity_uuid),
        wire_capture_sequence: Number(example.wire_capture_sequence),
        wire_observed_micros: Number(example.wire_observed_micros),
        instance_id: Number(rows[0].instance_id),
        state: String(rows[0].state),
        source_entity_uuid: Number(rows[0].source_entity_uuid),
        normalized_coefficient: raw / signed,
      });
    }
  }
  return occurrences;
}

function uniqueOccurrenceIndex(occurrences, label) {
  const index = new Map();
  for (const row of occurrences) {
    const key = [
      row.session_id,
      row.run_ordinal,
      row.target_entity_uuid,
      row.wire_capture_sequence,
    ].join("|");
    requireExact(!index.has(key), `${label} unique occurrence ${key}`);
    index.set(key, row);
  }
  return index;
}

function buildOwnershipIndex(ownership) {
  const index = new Map();
  for (const row of ownership.resolutions ?? []) {
    if (Number(row.effect_id) !== 2110140 || row.class !== "direct_player") continue;
    const source = row.source;
    if (source?.kind !== "player" || !source.character_id) continue;
    const key = [row.session_id, Number(row.run_ordinal), Number(source.entity_uuid)].join("|");
    const value = {
      character_id: String(source.character_id),
      class_id: source.class_id == null ? null : Number(source.class_id),
      specialization_id:
        source.specialization_id == null ? null : Number(source.specialization_id),
    };
    if (index.has(key)) {
      const prior = index.get(key);
      requireExact(prior.character_id === value.character_id, `${key} stable character identity`);
      if (prior.specialization_id == null && value.specialization_id != null) index.set(key, value);
    } else {
      index.set(key, value);
    }
  }
  return index;
}

function groupProviderEvidence(rows) {
  const groups = new Map();
  for (const row of rows) {
    const key = `${row.provider_entity_uuid}|${row.provider_character_id}|${row.loadout_tier}`;
    if (!groups.has(key)) {
      groups.set(key, {
        provider_entity_uuid: row.provider_entity_uuid,
        provider_character_id: row.provider_character_id,
        provider_class_id: row.provider_class_id,
        loadout_tier: row.loadout_tier,
        exact_attribute_pair: row.exact_attribute_pair,
        exact_occurrences: 0,
        independent_session_runs: new Set(),
        target_entity_uuids: new Set(),
      });
    }
    const group = groups.get(key);
    group.exact_occurrences += 1;
    group.independent_session_runs.add(`${row.session_id}|${row.run_ordinal}`);
    group.target_entity_uuids.add(row.target_entity_uuid);
  }
  return [...groups.values()].map((group) => ({
    provider_entity_uuid: group.provider_entity_uuid,
    provider_character_id: group.provider_character_id,
    provider_class_id: group.provider_class_id,
    loadout_tier: group.loadout_tier,
    exact_attribute_pair: group.exact_attribute_pair,
    exact_occurrences: group.exact_occurrences,
    independent_session_runs: group.independent_session_runs.size,
    target_entity_count: group.target_entity_uuids.size,
    resolution_scope: "retained exact lifecycle occurrences only",
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  })).sort((left, right) => left.provider_entity_uuid - right.provider_entity_uuid);
}

function verify(report) {
  requireExact(report.schema_version === 1, "report schema");
  requireExact(
    report.generated_by === "tools/rdps-imagine-status-attribute-tier-proof.mjs",
    "report generator",
  );
  requireExact(Number(report.effect_id) === 2110140, "report effect");
  requireExact(Number(report.imagine_skill_id) === 3971, "report skill");
  requireExact(report.policy?.tier_resolution_is_occurrence_scoped === true, "occurrence scope");
  requireExact(
    report.policy?.provider_tier_is_not_propagated_across_time_or_recipients === true,
    "non-propagation policy",
  );
  requireExact(report.policy?.provider_rdps_credit_allowed === false, "credit policy");
  requireExact(Number(report.summary?.exact_paired_attribute_occurrences) === 8, "exact pair count");
  requireExact(Number(report.summary?.exact_base_tier_occurrences) === 2, "base-tier count");
  requireExact(Number(report.summary?.exact_tier_5_occurrences) === 6, "tier-5 count");
  requireExact(Number(report.summary?.unresolved_applied_status_instances) === 128, "unresolved count");
  requireExact(Number(report.summary?.unmatched_clean_attribute_occurrences) === 0, "unmatched count");
  requireExact(Number(report.summary?.observed_damage_reassigned_to_provider) === 0, "conservation boundary");
  requireExact(
    (report.resolved_lifecycle_occurrences ?? []).every(
      (row) => row.formula_authority === false && row.runtime_authority === false &&
        row.provider_rdps_credit_allowed === false,
    ),
    "occurrence authority boundary",
  );
  console.log(
    `verified effect 2110140 tier proof for build ${report.game_build}: 8 exact, 128 unresolved, zero provider credit`,
  );
  return report;
}

function compareOccurrences(left, right) {
  return left.session_id.localeCompare(right.session_id) ||
    left.run_ordinal - right.run_ordinal ||
    left.wire_capture_sequence - right.wire_capture_sequence ||
    left.target_entity_uuid - right.target_entity_uuid;
}

function numberParameter(parameters, key) {
  const pair = (parameters ?? []).find((entry) => entry?.[0] === key);
  const value = Number(pair?.[1]);
  requireExact(Number.isFinite(value), `numeric parameter ${key}`);
  return value;
}

function receipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: filePath.replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function parseArguments(values) {
  const result = {};
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith("--") || value == null) {
      usage();
      process.exit(2);
    }
    result[key.slice(2)] = value;
  }
  return result;
}

function required(options, key) {
  if (!options[key]) throw new Error(`missing --${key}`);
  return options[key];
}

function requireExact(condition, label) {
  if (!condition) throw new Error(`${label} does not match the exact proof contract`);
}

function usage() {
  console.log(`Usage:
  node tools/rdps-imagine-status-attribute-tier-proof.mjs build --build <id> --skill-fight-level-table <json> --skill-aoyi-star-table <json> --status-attribute-proof <json> --provider-ownership-proof <json> --output <json>
  node tools/rdps-imagine-status-attribute-tier-proof.mjs verify --input <json>`);
}
