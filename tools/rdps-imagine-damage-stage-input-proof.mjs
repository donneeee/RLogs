#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATOR = "tools/rdps-imagine-damage-stage-input-proof.mjs";
const [command, ...args] = process.argv.slice(2);
if (command === "build") build(parseArguments(args));
else if (command === "verify") verify(readJson(path.resolve(required(parseArguments(args), "input"))));
else {
  usage();
  process.exitCode = 2;
}

function build(options) {
  const build = required(options, "build");
  const windowsPath = path.resolve(required(options, "tier-window-inputs"));
  const transitionsPath = path.resolve(required(options, "transition-proof"));
  const routesPath = path.resolve(required(options, "damage-route-proof"));
  const stageCandidatePath = path.resolve(required(options, "damage-stage-candidate"));
  const gameSchemaPath = path.resolve(required(options, "game-schema-source"));
  const tablePath = path.resolve(required(options, "damage-attr-table"));
  const outputPath = path.resolve(required(options, "output"));
  const windows = readJson(windowsPath);
  const transitions = readJson(transitionsPath);
  const routes = readJson(routesPath);
  const stageCandidate = readJson(stageCandidatePath);
  const gameSchemaSource = fs.readFileSync(gameSchemaPath, "utf8");
  const table = readJson(tablePath);

  exact(String(windows.game_build) === build && windows.schema_version === 1, "window build/schema");
  exact(Number(windows.inputs?.support_timeline?.schema_version) === 10, "stage-bearing timeline schema");
  exact(String(transitions.game_build) === build && transitions.schema_version === 1, "transition build/schema");
  exact(Number(transitions.summary?.effective_stat_window_damage_actions) === 12547, "effective action frontier");
  exact(String(routes.game_build) === build && Number(routes.schema_version) === 9, "route build/schema");
  exact(routes.generated_by === "rlogs-bpsr-damage-source-route-proof", "route generator");
  exact(Number(routes.summary?.lookup_keys) === 5678, "route lookup coverage");
  exact(String(stageCandidate.game_build) === build && Number(stageCandidate.schema_version) === 9, "stage candidate build/schema");
  exact(stageCandidate.generated_by === "rlogs-bpsr-damage-stage-runtime-catalog", "stage candidate generator");
  exact(stageCandidate.policy?.runtime_formula_authority === false, "stage candidate authority");
  exact(
    stageCandidate.policy?.coefficient_selection ===
      "one-value vectors are stage invariant; multi-value vectors use zero-based packet owner_stage and omitted owner_stage is zero",
    "stage candidate omitted-scalar semantics",
  );
  exact(
    gameSchemaSource.includes('#[prost(int32, optional, tag = "14")]') &&
      gameSchemaSource.includes("pub owner_stage: Option<i32>"),
    "current decoder optional owner-stage schema",
  );

  const routeIndex = uniqueMap(routes.keys ?? [], (row) => String(row.lookup_key), "route key");
  const transitionIndex = uniqueMap(
    transitions.lifecycle_windows ?? [],
    lifecycleKey,
    "transition lifecycle",
  );
  const selected = [];
  for (const window of windows.lifecycle_windows ?? []) {
    const transition = transitionIndex.get(lifecycleKey(window));
    exact(Boolean(transition), `transition ${lifecycleKey(window)}`);
    const eligible = new Set(
      transition.damage_action_classification.effective_canonical_source_rlog_sequences.map(Number),
    );
    for (const action of window.damage_actions ?? []) {
      if (!eligible.has(Number(action.canonical_source_rlog_sequence))) continue;
      selected.push(analyzeAction(window, action, routeIndex, table));
    }
  }
  exact(selected.length === 12547, "selected effective actions");
  const groups = groupActions(selected);
  const count = (predicate) => selected.filter(predicate).length;
  const report = {
    schema_version: 1,
    generated_by: GENERATOR,
    game_build: build,
    effect_id: 2110140,
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      damage_endpoint_allegiance: "unresolved",
      allegiance_assumptions: false,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      remote_player_cast_packets_required: false,
      owner_stage_is_zero_based_index_not_stage_type: true,
      missing_packet_owner_stage_is_preserved_as_null: true,
      omitted_optional_owner_stage_has_separate_semantic_zero_candidate: true,
      semantic_zero_candidate_is_not_a_synthesized_packet_field: true,
      missing_owner_level_is_unknown_not_zero: true,
      one_value_coefficient_vector_is_stage_invariant: true,
      populated_fixed_parameter_vector_uses_one_based_owner_level: true,
      empty_fixed_parameter_vector_is_retained_as_zero_input_candidate: true,
      row_identity_does_not_prove_damage_formula_or_operation_order: true,
      nonstandard_damage_scripts_are_not_reinterpreted_as_attack: true,
      integer_damage_counterfactual_complete: false,
      ordinary_damage_totals_changed: false,
      observed_damage_reassigned_to_provider: "0",
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      tier_window_inputs: receipt(windowsPath),
      primary_attack_transition_proof: receipt(transitionsPath),
      current_build_damage_route_proof: receipt(routesPath),
      current_build_damage_stage_candidate: receipt(stageCandidatePath),
      current_decoder_game_schema_source: receipt(gameSchemaPath),
      current_build_damage_attr_table: receipt(tablePath),
    },
    summary: {
      effective_stat_window_damage_actions: selected.length,
      unique_damage_stage_input_groups: groups.length,
      uniquely_routed_damage_attr_rows: count((row) => row.route_state === "unique"),
      unresolved_damage_attr_routes: count((row) => row.route_state !== "unique"),
      selected_coefficient_inputs: count((row) => row.coefficient_state === "selected"),
      selected_coefficient_inputs_from_packet_stage: count(
        (row) => row.coefficient_selection_evidence === "explicit-packet-owner-stage",
      ),
      selected_coefficient_inputs_from_optional_scalar_default: count(
        (row) => row.coefficient_selection_evidence === "optional-protobuf-scalar-semantic-zero",
      ),
      unresolved_coefficient_inputs: count((row) => row.coefficient_state !== "selected"),
      selected_fixed_parameter_inputs: count((row) => row.fixed_parameter_state === "selected"),
      unresolved_fixed_parameter_inputs: count((row) => row.fixed_parameter_state !== "selected"),
      standard_attack_script_actions: count((row) => row.damage_script === "Attack"),
      nonstandard_script_actions: count(
        (row) => row.damage_script != null && row.damage_script !== "Attack" && row.damage_script !== "MAttack",
      ),
      missing_packet_owner_stage: count((row) => row.owner_stage == null),
      missing_packet_owner_level: count((row) => row.owner_level == null),
      retained_hp_loss: sum(selected, "hp_loss"),
      retained_reported_damage: sum(selected, "reported_amount"),
      observed_damage_reassigned_to_provider: "0",
    },
    damage_script_counts: countsBy(selected, (row) => row.damage_script ?? "<unrouted>"),
    damage_stage_input_groups: groups,
    remaining_proof_obligations: [
      "resolve the 125 actions without an exact current-build DamageAttr route",
      "promote or reject optional owner_stage semantic-zero selection through exact current-client/server formula replay; raw absence remains preserved",
      "prove the exact Attack and nonstandard-script formula stages, stat inputs, stacking, and integer operation order",
      "resolve the one same-packet attack-percent confounder at effect activation",
      "replay each eligible damage action with and without the provider-owned marginal and conserve recipient debit/provider credit",
      "satisfy canonical-replay-conservation and protocol-event-coverage with the exact-build protocol-pack identity",
    ],
  };
  verify(report);
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(
    `wrote ${outputPath}: ${report.summary.uniquely_routed_damage_attr_rows}/${selected.length} routed, ${report.summary.selected_coefficient_inputs} coefficient inputs selected, zero provider credit`,
  );
}

function analyzeAction(window, action, routeIndex, table) {
  const lookupKey = `${action.action_id}:${action.hit_event_id ?? 0}`;
  const route = routeIndex.get(lookupKey);
  let candidate = null;
  let routeState = "missing-lookup-key";
  if (route?.candidates?.length === 1) {
    candidate = route.candidates[0];
    routeState = "unique";
  } else if ((route?.candidates?.length ?? 0) > 1 && action.damage_source != null) {
    const selections = (route.selection_by_damage_source ?? []).filter(
      (row) => Number(row.damage_source_id) === Number(action.damage_source),
    );
    if (selections.length === 1) {
      candidate = route.candidates.find(
        (row) => Number(row.damage_attr_id) === Number(selections[0].damage_attr_id),
      );
      routeState = candidate ? "unique" : "source-selection-missing-candidate";
    } else routeState = "ambiguous-source-selection";
  } else if (route) routeState = "ambiguous-route";

  const damageAttrId = candidate == null ? null : String(candidate.damage_attr_id);
  const damageRow = damageAttrId == null ? null : table[damageAttrId];
  exact(damageAttrId == null || Number(damageRow?.Id) === Number(damageAttrId), `DamageAttr row ${damageAttrId}`);
  const coefficients = (damageRow?.PVEDamageRadio ?? []).map(Number);
  const fixedParameters = (damageRow?.PVEFixedParameter ?? []).map(Number);
  const coefficient = selectCoefficient(coefficients, action.owner_stage);
  const fixed = selectFixed(fixedParameters, action.owner_level);
  return {
    session_id: window.session_id,
    status_instance_id: Number(window.status_instance_id),
    provider_entity_uuid: String(window.provider_entity_uuid),
    affected_entity_uuid: String(window.affected_entity_uuid),
    sequence: Number(action.canonical_source_rlog_sequence),
    recipient_or_enemy_target_entity_uuid: String(action.recipient_or_enemy_target_entity_uuid),
    damage_endpoint_allegiance: "unresolved",
    action_id: action.action_id == null ? null : Number(action.action_id),
    hit_event_id: action.hit_event_id == null ? null : Number(action.hit_event_id),
    damage_source: action.damage_source == null ? null : Number(action.damage_source),
    owner_level: action.owner_level == null ? null : Number(action.owner_level),
    owner_stage: action.owner_stage == null ? null : Number(action.owner_stage),
    route_state: routeState,
    damage_attr_id: damageAttrId,
    damage_script: damageRow?.DamageScript == null ? null : String(damageRow.DamageScript),
    coefficient_state: damageRow == null ? "unrouted" : coefficient.state,
    coefficient_selection_evidence: damageRow == null ? "unrouted" : coefficient.evidence,
    coefficient_basis_points: coefficient.value,
    coefficient_vector_length: coefficients.length,
    fixed_parameter_state: damageRow == null ? "unrouted" : fixed.state,
    fixed_parameter: fixed.value,
    fixed_parameter_vector_length: fixedParameters.length,
    reported_amount: String(action.reported_amount ?? "0"),
    hp_loss: String(action.hp_loss ?? "0"),
    provider_rdps_credit: "0",
  };
}

function selectCoefficient(values, stage) {
  if (values.length === 0) return { state: "missing-vector", value: null, evidence: "missing-vector" };
  if (values.length === 1) {
    return { state: "selected", value: values[0], evidence: "stage-invariant-one-value-vector" };
  }
  const index = stage == null ? 0 : Number(stage);
  return Number.isInteger(index) && index >= 0 && index < values.length
    ? {
      state: "selected",
      value: values[index],
      evidence: stage == null
        ? "optional-protobuf-scalar-semantic-zero"
        : "explicit-packet-owner-stage",
    }
    : { state: "owner-stage-out-of-range", value: null, evidence: "owner-stage-out-of-range" };
}

function selectFixed(values, level) {
  if (values.length === 0) return { state: "selected", value: 0 };
  if (level == null) return { state: "missing-owner-level", value: null };
  const index = Number(level) - 1;
  return Number.isInteger(index) && index >= 0 && index < values.length
    ? { state: "selected", value: values[index] }
    : { state: "owner-level-out-of-range", value: null };
}

function groupActions(rows) {
  const groups = new Map();
  for (const row of rows) {
    const key = JSON.stringify([
      row.action_id, row.hit_event_id, row.damage_source, row.owner_level, row.owner_stage,
      row.route_state, row.damage_attr_id, row.damage_script, row.coefficient_state,
      row.coefficient_basis_points, row.fixed_parameter_state, row.fixed_parameter,
      row.coefficient_selection_evidence,
    ]);
    if (!groups.has(key)) {
      groups.set(key, {
        action_id: row.action_id,
        hit_event_id: row.hit_event_id,
        damage_source: row.damage_source,
        owner_level: row.owner_level,
        owner_stage: row.owner_stage,
        route_state: row.route_state,
        damage_attr_id: row.damage_attr_id,
        damage_script: row.damage_script,
        coefficient_state: row.coefficient_state,
        coefficient_basis_points: row.coefficient_basis_points,
        coefficient_selection_evidence: row.coefficient_selection_evidence,
        coefficient_vector_length: row.coefficient_vector_length,
        fixed_parameter_state: row.fixed_parameter_state,
        fixed_parameter: row.fixed_parameter,
        fixed_parameter_vector_length: row.fixed_parameter_vector_length,
        event_count: 0,
        hp_loss: 0n,
        reported_damage: 0n,
        first_sequence: row.sequence,
        last_sequence: row.sequence,
        endpoint_entity_uuids: new Set(),
      });
    }
    const group = groups.get(key);
    group.event_count += 1;
    group.hp_loss += BigInt(row.hp_loss);
    group.reported_damage += BigInt(row.reported_amount);
    group.first_sequence = Math.min(group.first_sequence, row.sequence);
    group.last_sequence = Math.max(group.last_sequence, row.sequence);
    group.endpoint_entity_uuids.add(row.recipient_or_enemy_target_entity_uuid);
  }
  return [...groups.values()].map((group) => ({
    ...group,
    hp_loss: group.hp_loss.toString(),
    reported_damage: group.reported_damage.toString(),
    endpoint_count: group.endpoint_entity_uuids.size,
    endpoint_entity_uuids: undefined,
    damage_endpoint_allegiance: "unresolved",
    formula_authority: false,
    provider_rdps_credit: "0",
  })).sort((a, b) => (a.action_id ?? -1) - (b.action_id ?? -1) || (a.hit_event_id ?? -1) - (b.hit_event_id ?? -1));
}

function verify(report) {
  exact(report.schema_version === 1 && report.generated_by === GENERATOR, "report schema/generator");
  exact(Number(report.effect_id) === 2110140, "report effect");
  exact(report.topology?.allegiance_assumptions === false, "neutral topology");
  exact(report.policy?.remote_player_cast_packets_required === false, "remote cast policy");
  exact(report.policy?.missing_packet_owner_stage_is_preserved_as_null === true, "missing stage preservation");
  exact(report.policy?.omitted_optional_owner_stage_has_separate_semantic_zero_candidate === true, "semantic zero candidate");
  exact(report.policy?.formula_authority === false, "formula authority");
  exact(report.policy?.provider_rdps_credit_allowed === false, "credit policy");
  exact(Number(report.summary?.effective_stat_window_damage_actions) === 12547, "action count");
  exact(Number(report.summary?.uniquely_routed_damage_attr_rows) === 12422, "routed count");
  exact(Number(report.summary?.unresolved_damage_attr_routes) === 125, "unrouted count");
  exact(Number(report.summary?.selected_coefficient_inputs) === 12353, "coefficient count");
  exact(Number(report.summary?.selected_coefficient_inputs_from_packet_stage) === 490, "explicit stage count");
  exact(Number(report.summary?.selected_coefficient_inputs_from_optional_scalar_default) === 4981, "semantic zero count");
  exact(Number(report.summary?.unresolved_coefficient_inputs) === 194, "unresolved coefficient count");
  exact(Number(report.summary?.selected_fixed_parameter_inputs) === 12389, "fixed count");
  exact(Number(report.summary?.unresolved_fixed_parameter_inputs) === 158, "unresolved fixed count");
  exact(Number(report.summary?.standard_attack_script_actions) === 12116, "standard attack count");
  exact(Number(report.summary?.nonstandard_script_actions) === 306, "nonstandard count");
  exact(String(report.summary?.observed_damage_reassigned_to_provider) === "0", "zero reassignment");
  const groups = report.damage_stage_input_groups ?? [];
  exact(groups.length === Number(report.summary.unique_damage_stage_input_groups), "group count");
  exact(groups.every((row) => row.damage_endpoint_allegiance === "unresolved" && row.formula_authority === false && row.provider_rdps_credit === "0"), "group authority");
  console.log(
    `verified effect 2110140 damage-stage inputs for build ${report.game_build}: 12422/12547 uniquely routed, 12353 coefficient inputs selected (4981 via preserved-null semantic-zero candidate), zero provider credit`,
  );
}

function lifecycleKey(row) {
  return `${row.session_id}|${Number(row.status_instance_id)}`;
}

function uniqueMap(rows, keyOf, label) {
  const map = new Map();
  for (const row of rows) {
    const key = keyOf(row);
    exact(!map.has(key), `${label} ${key}`);
    map.set(key, row);
  }
  return map;
}

function countsBy(rows, keyOf) {
  const counts = {};
  for (const row of rows) counts[keyOf(row)] = (counts[keyOf(row)] ?? 0) + 1;
  return counts;
}

function sum(rows, field) {
  return rows.reduce((total, row) => total + BigInt(row[field] ?? "0"), 0n).toString();
}

function receipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return { path: filePath.replaceAll("\\", "/"), bytes: bytes.length, sha256: crypto.createHash("sha256").update(bytes).digest("hex") };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function parseArguments(values) {
  const result = {};
  for (let i = 0; i < values.length; i += 2) {
    if (!values[i]?.startsWith("--") || values[i + 1] == null) {
      usage();
      process.exit(2);
    }
    result[values[i].slice(2)] = values[i + 1];
  }
  return result;
}

function required(options, key) {
  if (!options[key]) throw new Error(`missing --${key}`);
  return options[key];
}

function exact(condition, label) {
  if (!condition) throw new Error(`${label} does not match the exact proof contract`);
}

function usage() {
  console.log(`Usage:
  node ${GENERATOR} build --build <id> --tier-window-inputs <json> --transition-proof <json> --damage-route-proof <json> --damage-stage-candidate <json> --game-schema-source <rs> --damage-attr-table <json> --output <json>
  node ${GENERATOR} verify --input <json>`);
}
