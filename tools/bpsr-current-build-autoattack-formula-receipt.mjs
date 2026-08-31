import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-current-build-autoattack-formula-receipt.mjs";

function fail(message) {
  throw new Error(message);
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) fail(`invalid option near ${key ?? "<end>"}`);
    options[key.slice(2)] = value;
  }
  return options;
}

function required(options, key) {
  const value = options[key];
  if (!value) fail(`missing --${key}`);
  return value;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function contentSha256(value) {
  return sha256(Buffer.from(JSON.stringify(stable(value)), "utf8"));
}

function source(pathValue) {
  const absolute = path.resolve(pathValue);
  const bytes = readFileSync(absolute);
  return {
    absolute,
    path: path.relative(process.cwd(), absolute).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: sha256(bytes),
    value: JSON.parse(bytes.toString("utf8")),
  };
}

function sourceReceipt(entry) {
  return { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 };
}

function exactBuild(value, gameBuild) {
  return [value.game_build, value.client_build, value.build_id]
    .filter((entry) => typeof entry === "string")
    .some((entry) => entry === gameBuild || entry.endsWith(`-${gameBuild}`));
}

function buildReceipt(options) {
  const gameBuild = required(options, "build");
  const carryForward = source(required(options, "carry-forward"));
  const formulaSurface = source(required(options, "formula-surface"));
  const operator = source(required(options, "operator-frontier"));
  const clientOperator = source(required(options, "client-operator-frontier"));
  const rounding = source(required(options, "rounding-worklist"));
  const runtime = source(required(options, "runtime"));

  assert.equal(exactBuild(carryForward.value, gameBuild), true, "carry-forward build");
  assert.equal(exactBuild(formulaSurface.value, gameBuild), true, "formula-surface build");
  assert.equal(exactBuild(operator.value, gameBuild), true, "operator-frontier build");
  assert.equal(exactBuild(clientOperator.value, gameBuild), true, "client-operator-frontier build");
  assert.equal(exactBuild(rounding.value, gameBuild), true, "rounding-worklist build");
  assert.equal(exactBuild(runtime.value, gameBuild), true, "runtime build");
  assert.equal(carryForward.value.policy?.byte_identical_static_tables_are_current_build_evidence, true);
  assert.equal(carryForward.value.policy?.current_build_packet_replay_required, true);
  assert.equal(carryForward.value.policy?.runtime_promotion_allowed, false);

  const operatorConclusion = operator.value.conclusion;
  assert.equal(operatorConclusion?.one_skill_identity_and_topology_reconstructed, true);
  assert.equal(operatorConclusion?.exact_autoattack_row_coefficient_plus_fixed_relation_proven, true);
  assert.equal(operatorConclusion?.exact_autoattack_stat_lane_proven, true);
  assert.equal(operatorConclusion?.exact_autoattack_operator_proven, false);
  assert.equal(operatorConclusion?.exact_integer_rounding_proven, false);
  assert.equal(operatorConclusion?.exact_provider_counterfactual_proven, false);
  assert.equal(clientOperator.value.conclusion?.exact_static_coefficient_fixed_relation_proven, true);
  assert.equal(clientOperator.value.conclusion?.authoritative_client_damage_operator_found, false);
  assert.equal(clientOperator.value.conclusion?.authoritative_server_damage_operator_found, false);
  assert.equal(clientOperator.value.conclusion?.exact_integer_rounding_proven, false);
  assert.equal(clientOperator.value.conclusion?.provider_rdps_credit_allowed, false);
  if (Number(clientOperator.value.schema_version) >= 2) {
    assert.equal(clientOperator.value.conclusion?.retained_client_journal_frontier_exhausted, true);
    assert.ok(clientOperator.value.retained_client_journal_search?.journal_files > 0);
    assert.ok(clientOperator.value.retained_client_journal_search?.client_packet_records > 0);
    assert.equal(
      clientOperator.value.retained_client_journal_search?.client_payloads_with_ability_varint,
      0,
    );
    assert.equal(clientOperator.value.retained_client_journal_search?.ability_varint_occurrences, 0);
  }
  assert.equal(rounding.value.conclusion?.current_exact_controlled_pairs_available, 0);
  assert.equal(rounding.value.conclusion?.acquisition_ready, true);
  assert.equal(rounding.value.conclusion?.exact_integer_rounding_proven, false);
  assert.equal(rounding.value.conclusion?.provider_rdps_credit_allowed, false);
  assert.ok(Number(rounding.value.schema_version) >= 3);
  const qualification = rounding.value.controlled_pair_qualification_funnel;
  assert.equal(qualification?.authority, "diagnostic_only_no_proof_rules_relaxed");
  assert.equal(
    qualification?.independent_direct_source_trials_current_hp_normalized
      ?.first_stage_with_no_cross_state_pairs,
    "same_target_status_state",
  );
  assert.equal(runtime.value.promotion_state, "blocked-current-build-proof-gates-open");
  assert.equal(runtime.value.policy?.candidate_rules_enabled, false);
  assert.equal(runtime.value.policy?.runtime_promotion_allowed, false);

  const rows = operator.value.static_route?.exact_damage_attr_rows;
  assert.ok(Array.isArray(rows) && rows.length > 0, "exact DamageAttr rows");
  const selectedStat = operator.value.autoattack_operator_frontier?.exact_stat_lane;
  const coefficientSelection = operator.value.autoattack_operator_frontier?.exact_coefficient_selection;
  assert.equal(selectedStat?.exact_stat_lane_proven, true);
  assert.equal(selectedStat?.selected_attribute_id, 11330);
  assert.equal(coefficientSelection?.exact_coefficient_selection_proven, true);

  const receipt = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    deployment_id: "global",
    channel: "steam",
    promotion_state: "candidate-static-formula-rounding-open",
    identity: {
      ability_id: 2900840,
      damage_script: "AutoAttack",
      selected_stat: selectedStat.selected_stat,
      selected_attribute_id: selectedStat.selected_attribute_id,
      packet_damage_mode: selectedStat.packet_damage_mode,
      protocol_pack_digest: runtime.value.protocol_pack_digest,
    },
    policy: {
      exact_numeric_ids_are_runtime_keys: true,
      localized_formula_text_is_supporting_evidence_only: true,
      current_character_snapshots_are_never_substituted: true,
      historical_packet_evidence_is_not_current_build_authority: true,
      unresolved_operator_and_rounding_are_retained: true,
      ordinary_damage_is_unchanged: true,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
    sources: {
      formula_proof_carry_forward: sourceReceipt(carryForward),
      current_build_formula_surface: sourceReceipt(formulaSurface),
      one_skill_operator_frontier: sourceReceipt(operator),
      exact_build_client_operator_frontier: sourceReceipt(clientOperator),
      rounding_acquisition_worklist: sourceReceipt(rounding),
      current_runtime_gate: sourceReceipt(runtime),
    },
    proven_formula_surface: {
      relation: "PVEDamageRadio x Physical Attack + PVEFixedParameter",
      coefficient_denominator: 10000,
      selected_stat_attribute_id: 11330,
      damage_attr_rows: rows.map((row) => ({
        hit_event_id: row.hit_event_id,
        damage_attr_id: row.damage_attr_id,
        damage_type: row.damage_type,
        coefficient_basis_points_by_stage: row.coefficient_basis_points_by_stage,
        fixed_parameter_by_level: row.fixed_parameter_by_level,
      })),
      selected_coefficient_basis_points_by_hit_event_id:
        coefficientSelection.selected_coefficient_basis_points_by_hit_event_id,
      coefficient_selection_proven: true,
      fixed_addition_order_proven: true,
      physical_attack_lane_proven: true,
    },
    open_integer_boundary: {
      exact_server_operator_proven: false,
      authoritative_client_operator_found: false,
      exact_integer_rounding_proven: false,
      exact_controlled_pairs_available: 0,
      candidate_boundaries: ["floor", "ceil", "nearest_half_up", "unrounded_rational"],
      downstream_factor_cancellation_proven: false,
      exact_provider_counterfactual_proven: false,
      smallest_next_proof: clientOperator.value.conclusion.smallest_next_proof,
      ...(Number(clientOperator.value.schema_version) >= 2 ? {
        retained_client_request_frontier: {
          exhausted: true,
          journal_files: clientOperator.value.retained_client_journal_search.journal_files,
          total_bytes: clientOperator.value.retained_client_journal_search.total_bytes,
          client_packet_records:
            clientOperator.value.retained_client_journal_search.client_packet_records,
          client_application_bytes:
            clientOperator.value.retained_client_journal_search.client_application_bytes,
          client_payloads_with_ability_varint: 0,
          ability_varint_occurrences: 0,
        },
      } : {}),
      controlled_capture_frontier: {
        retained_owner_samples:
          rounding.value.current_cohort_pair_search.selected_samples,
        same_run_same_source_target_pairs_before_direct_instance_control:
          qualification.exact_direct_source_instance.stages.find(
            (stage) => stage.id === "same_source_and_target_entities",
          ).cross_state_pairs,
        independent_trials_with_same_packet_formula_context:
          qualification.independent_direct_source_trials.stages.find(
            (stage) => stage.id === "same_packet_formula_context",
          ).cross_state_pairs,
        current_hp_normalized_pairs_with_same_target_attributes:
          qualification.independent_direct_source_trials_current_hp_normalized.stages.find(
            (stage) => stage.id === "same_target_attribute_state",
          ).cross_state_pairs,
        next_blocking_equal_fields:
          qualification.independent_direct_source_trials_current_hp_normalized
            .blocking_equal_fields,
        required_experiment:
          "replicated numeric monster-3000043 summon trials before, during, and after one exact effect-2110140 lifecycle with unchanged target statuses and all other controlled context",
      },
    },
    conclusion: {
      current_build_formula_catalog_entry: true,
      exact_static_formula_inputs_proven: true,
      exact_client_operator_search_completed: true,
      ...(Number(clientOperator.value.schema_version) >= 2
        ? { retained_client_journal_frontier_exhausted: true }
        : {}),
      authoritative_client_operator_found: false,
      final_server_integer_boundary_open: true,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
  };
  return { ...receipt, content_sha256: contentSha256(receipt) };
}

function validateReceipt(value) {
  assert.equal(value.schema_version, SCHEMA_VERSION);
  assert.equal(value.generated_by, GENERATED_BY);
  assert.equal(value.policy.exact_numeric_ids_are_runtime_keys, true);
  assert.equal(value.policy.current_character_snapshots_are_never_substituted, true);
  assert.equal(value.policy.unresolved_operator_and_rounding_are_retained, true);
  assert.equal(value.policy.provider_rdps_credit_allowed, false);
  assert.equal(value.policy.runtime_promotion_allowed, false);
  assert.equal(value.conclusion.current_build_formula_catalog_entry, true);
  assert.equal(value.conclusion.exact_static_formula_inputs_proven, true);
  assert.equal(value.conclusion.final_server_integer_boundary_open, true);
  assert.equal(value.conclusion.provider_rdps_credit_allowed, false);
  const withoutHash = structuredClone(value);
  delete withoutHash.content_sha256;
  assert.equal(value.content_sha256, contentSha256(withoutHash));
}

function optionsFromReceipt(value, output) {
  return {
    build: value.game_build,
    "carry-forward": value.sources.formula_proof_carry_forward.path,
    "formula-surface": value.sources.current_build_formula_surface.path,
    "operator-frontier": value.sources.one_skill_operator_frontier.path,
    "client-operator-frontier": value.sources.exact_build_client_operator_frontier.path,
    "rounding-worklist": value.sources.rounding_acquisition_worklist.path,
    runtime: value.sources.current_runtime_gate.path,
    output,
  };
}

function generate(options) {
  const output = path.resolve(required(options, "output"));
  const receipt = buildReceipt(options);
  validateReceipt(receipt);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(receipt, null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const input = path.resolve(required(options, "input"));
  const value = JSON.parse(readFileSync(input, "utf8"));
  validateReceipt(value);
  const rebuilt = buildReceipt(optionsFromReceipt(value, input));
  assert.deepEqual(value, rebuilt);
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-current-build-autoattack-formula-receipt.mjs generate --build <id> --carry-forward <json> --formula-surface <json> --operator-frontier <json> --client-operator-frontier <json> --rounding-worklist <json> --runtime <json> --output <json>\n  node tools/bpsr-current-build-autoattack-formula-receipt.mjs verify --input <json>");
  process.exit(1);
}
