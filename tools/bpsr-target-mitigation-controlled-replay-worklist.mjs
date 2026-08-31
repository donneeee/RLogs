#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATOR = "tools/bpsr-target-mitigation-controlled-replay-worklist.mjs";
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(options);
else if (command === "verify") verify(readJson(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(parsed) {
  const buildId = required(parsed, "build");
  const worklistPath = path.resolve(required(parsed, "worklist"));
  const identityPath = path.resolve(required(parsed, "action-identity"));
  const offlinePath = path.resolve(required(parsed, "offline-exhaustion-proof"));
  const outputPath = path.resolve(required(parsed, "output"));
  const worklist = readJson(worklistPath);
  const identity = readJson(identityPath);
  const offline = readJson(offlinePath);
  validateInputs(buildId, worklist, identity, offline);

  const identities = new Map(identity.observations.map((row) => [key(row), row]));
  const axisValues = new Map([[11350, new Map()], [11360, new Map()], [11420, new Map()]]);
  const contexts = new Map();
  let joined = 0;
  for (const action of worklist.observations) {
    const actor = identities.get(key(action));
    if (!actor || Number(actor.target_entity_uuid) !== Number(action.target_entity_uuid) ||
      Number(actor.source_entity_uuid) !== Number(action.source_entity_uuid)) {
      throw new Error(`action identity mismatch at ${key(action)}`);
    }
    joined += 1;
    const vector = Object.fromEntries(
      (action.observed_mitigation_attributes ?? []).map((row) => [String(row.attribute_id), Number(row.value)]),
    );
    for (const [attributeId, values] of axisValues) {
      if (vector[attributeId] != null) increment(values, String(vector[attributeId]));
    }
    const context = {
      scene_id: actor.scene_id ?? null,
      source_numeric_monster_id: actor.source_numeric_monster_id ?? null,
      source_actor_kind: actor.source_actor_kind ?? null,
      target_actor_kind: actor.actor_kind ?? null,
      target_class_id: actor.class_id ?? null,
      target_specialization_id: actor.specialization_id ?? null,
      ability_id: Number(action.ability_id),
      hit_event_id: action.hit_event_id ?? null,
      damage_source: action.damage_source ?? null,
      damage_type: action.damage_type ?? null,
      packet_property: action.packet_property ?? null,
    };
    const contextKey = JSON.stringify(context);
    const bucket = contexts.get(contextKey) ?? {
      ...context,
      actions: 0,
      sessions: new Set(),
      defense_vectors: new Map(),
      source_status_state_ids: new Set(),
      target_status_state_ids: new Set(),
    };
    bucket.actions += 1;
    bucket.sessions.add(String(action.session_id));
    increment(bucket.defense_vectors, JSON.stringify(vector));
    bucket.source_status_state_ids.add(Number(action.source_status_state_id));
    bucket.target_status_state_ids.add(Number(action.target_status_state_id));
    contexts.set(contextKey, bucket);
  }

  const rankedContexts = [...contexts.values()]
    .sort((left, right) => right.actions - left.actions || compareJson(left, right))
    .slice(0, 40)
    .map((row) => ({
      scene_id: row.scene_id,
      source_numeric_monster_id: row.source_numeric_monster_id,
      source_actor_kind: row.source_actor_kind,
      target_actor_kind: row.target_actor_kind,
      target_class_id: row.target_class_id,
      target_specialization_id: row.target_specialization_id,
      ability_id: row.ability_id,
      hit_event_id: row.hit_event_id,
      damage_source: row.damage_source,
      damage_type: row.damage_type,
      packet_property: row.packet_property,
      actions: row.actions,
      sessions: [...row.sessions].sort(),
      distinct_defense_vectors: row.defense_vectors.size,
      defense_vectors: counts(row.defense_vectors),
      distinct_source_status_states: row.source_status_state_ids.size,
      distinct_target_status_states: row.target_status_state_ids.size,
      controlled_pair_already_available: false,
    }));

  const report = {
    schema_version: 1,
    generated_by: GENERATOR,
    game_build: buildId,
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "damage actor -> numeric action -> recipient or enemy target",
      effect_endpoint_allegiance_assumed: false,
      damage_endpoint_allegiance_assumed: false,
      lifecycle_to_damage_join_requires_exact_event_time_evidence: true,
    },
    policy: {
      exact_numeric_build_actor_action_and_attribute_ids_are_authoritative: true,
      target_allegiance_assumed: false,
      current_actor_or_character_snapshot_substituted: false,
      remote_player_packets_required: false,
      historical_near_pairs_are_controlled_formula_proof: false,
      client_ui_transform_is_combat_formula_authority: false,
      candidate_constants_are_runtime_authority: false,
      unresolved_evidence_is_hidden: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      mitigation_action_worklist: descriptor(worklistPath),
      event_time_action_identity: descriptor(identityPath),
      offline_client_and_packet_exhaustion: descriptor(offlinePath),
    },
    summary: {
      exact_actions_joined: joined,
      exact_player_targets: identity.summary.observations_with_active_actor_state,
      exact_event_time_damage_actors: identity.summary.observations_with_active_source_actor_state,
      exact_event_time_scenes: identity.observations.filter((row) => row.scene_id != null).length,
      physical_defense_values: axisValues.get(11350).size,
      magic_defense_values: axisValues.get(11360).size,
      refined_defense_values: axisValues.get(11420).size,
      controlled_axis_pairs_available: 0,
      client_combat_damage_consumer_proven: false,
      formulas_promoted: 0,
    },
    observed_axis_values: {
      physical_defense_11350: counts(axisValues.get(11350)),
      magic_defense_11360: counts(axisValues.get(11360)),
      refined_defense_11420: counts(axisValues.get(11420)),
    },
    ranked_exact_action_contexts: rankedContexts,
    controlled_replay_contract: {
      topology: "damage actor -> numeric action -> recipient or enemy target",
      invariant_fields: [
        "exact build and protocol pack", "same run scene", "same damage actor numeric identity",
        "same target actor and class/specialization", "same numeric ability and hit event",
        "same damage source/type/property and packet calculation identity",
        "same complete source attributes/statuses", "same complete target statuses",
        "same complete target attributes except exactly one selected mitigation family",
        "same critical/lucky/normal flags and owner level/stage", "same HP/shield pre-state",
      ],
      required_variants: [
        { axis_attribute_id: 11350, candidates: [6500, 22000], requirement: "two or more isolated physical-defense values" },
        { axis_attribute_id: 11360, candidates: [6500, 22000], requirement: "two or more isolated magic-defense values" },
        { axis_attribute_id: 11420, candidates: [6500, 9980], requirement: "two or more isolated refined-defense values" },
        { axis_attribute_id: 13200, candidates: [11000], requirement: "two or more isolated element-resistance values with exact packet property" },
      ],
      acceptance: "at least one deterministic divergent pair must accept one exact integer model and reject alternatives; repeated trials must prove server rounding and conservation",
      rejection: "any status, non-axis attribute, actor identity, scene, calculation, HP/shield, or output nondeterminism difference keeps the pair diagnostic only",
    },
    authority: {
      exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
  report.content_sha256 = contentHash(report);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(readJson(outputPath));
  console.log(`wrote ${outputPath}`);
}

function validateInputs(buildId, worklist, identity, offline) {
  if (Number(worklist?.schema_version) !== 1 ||
    worklist?.generated_by !== "rlogs-bpsr-target-mitigation-transform-proof:target-identity-worklist" ||
    String(worklist?.game_build) !== buildId || (worklist?.observations ?? []).length !== 774 ||
    worklist?.policy?.target_allegiance_assumed !== false ||
    worklist?.policy?.current_actor_snapshots_are_substituted !== false ||
    Number(identity?.schema_version) !== 2 ||
    identity?.generated_by !== "rlogs-bpsr-selected-action-target-identity-proof" ||
    String(identity?.game_build) !== buildId || (identity?.observations ?? []).length !== 774 ||
    Number(offline?.schema_version) !== 4 ||
    offline?.generated_by !== "tools/target-mitigation-offline-exhaustion-proof.mjs" ||
    String(offline?.game_build) !== buildId ||
    offline?.policy?.exact_client_combat_damage_consumer_proven !== false ||
    Number(offline?.summary?.neutral_player_targets) !== 774 ||
    Number(offline?.summary?.controlled_counterfactual_pairs) !== 0) {
    throw new Error("controlled replay worklist inputs are unsafe or incomplete");
  }
}

function verify(report) {
  if (Number(report?.schema_version) !== 1 || report?.generated_by !== GENERATOR ||
    report?.content_sha256 !== contentHash(report) ||
    report?.topology?.effect_edge !==
      "provider -> effect/status lifecycle -> recipient or enemy target" ||
    report?.topology?.damage_edge !==
      "damage actor -> numeric action -> recipient or enemy target" ||
    report?.topology?.effect_endpoint_allegiance_assumed !== false ||
    report?.topology?.damage_endpoint_allegiance_assumed !== false ||
    report?.topology?.lifecycle_to_damage_join_requires_exact_event_time_evidence !== true ||
    report?.policy?.target_allegiance_assumed !== false ||
    report?.policy?.current_actor_or_character_snapshot_substituted !== false ||
    report?.policy?.formula_authority !== false ||
    Number(report?.summary?.exact_actions_joined) !== 774 ||
    Number(report?.summary?.exact_player_targets) !== 774 ||
    Number(report?.summary?.controlled_axis_pairs_available) !== 0 ||
    Number(report?.summary?.formulas_promoted) !== 0 ||
    report?.authority?.exact_target_mitigation_formula_proven !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(report?.ranked_exact_action_contexts) ||
    report.ranked_exact_action_contexts.length === 0) {
    throw new Error("controlled replay worklist violates its fail-closed schema");
  }
  console.log(`target mitigation controlled replay worklist verified: ${report.summary.exact_actions_joined} exact actions, zero formulas promoted`);
}

function descriptor(file) {
  const bytes = fs.readFileSync(file);
  return { path: file.replaceAll("\\", "/"), bytes: bytes.length, sha256: crypto.createHash("sha256").update(bytes).digest("hex") };
}
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return crypto.createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function counts(map) { return [...map].map(([value, count]) => ({ value, count })).sort((a, b) => b.count - a.count || a.value.localeCompare(b.value)); }
function increment(map, keyValue) { map.set(keyValue, (map.get(keyValue) ?? 0) + 1); }
function key(row) { return `${row.session_id}:${row.run_ordinal}:${row.sequence}:${row.target_entity_uuid}`; }
function compareJson(left, right) { return JSON.stringify(left).localeCompare(JSON.stringify(right)); }
function readJson(file) { return JSON.parse(fs.readFileSync(path.resolve(file), "utf8")); }
function required(parsed, name) { const value = parsed[name]?.[0]; if (!value) throw new Error(`missing --${name}`); return value; }
function parseArgs(args) { const parsed = {}; for (let i = 0; i < args.length; i += 2) { const keyValue = args[i]?.replace(/^--/, ""); const value = args[i + 1]; if (!keyValue || value == null) throw new Error(`invalid argument near ${args[i]}`); (parsed[keyValue] ??= []).push(value); } return parsed; }
function selfTest() { const values = new Map(); increment(values, "5"); increment(values, "5"); if (counts(values)[0]?.count !== 2) throw new Error("count self-test failed"); console.log("bpsr-target-mitigation-controlled-replay-worklist self-test passed"); }
function usage(code) { console.log("Usage:\n  node tools/bpsr-target-mitigation-controlled-replay-worklist.mjs build --build <id> --worklist <json> --action-identity <json> --offline-exhaustion-proof <json> --output <json>\n  node tools/bpsr-target-mitigation-controlled-replay-worklist.mjs verify --input <json>\n  node tools/bpsr-target-mitigation-controlled-replay-worklist.mjs self-test"); process.exit(code); }
