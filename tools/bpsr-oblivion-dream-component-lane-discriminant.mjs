import { createHash } from "node:crypto";
import { readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const EFFECT_ID = 3_003_012;
const BUILD = "24687926";

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function parseArgs(argv) {
  const parsed = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    assert(key?.startsWith("--") && argv[index + 1], `invalid argument near ${key ?? "<end>"}`);
    parsed[key.slice(2)] = argv[index + 1];
  }
  return parsed;
}

function required(parsed, key) {
  const value = parsed[key];
  assert(value, `missing --${key}`);
  return path.resolve(value);
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`could not read ${label} ${file}: ${error.message}`);
  }
}

function descriptor(file) {
  const bytes = readFileSync(file);
  return {
    path: file,
    bytes: statSync(file).size,
    sha256: `sha256:${createHash("sha256").update(bytes).digest("hex")}`,
  };
}

function only(rows, label) {
  assert(rows.length === 1, `expected one ${label}, found ${rows.length}`);
  return rows[0];
}

function collectEffectObjects(value, output = []) {
  if (Array.isArray(value)) {
    for (const row of value) collectEffectObjects(row, output);
  } else if (value && typeof value === "object") {
    if (Number(value.effect_id) === EFFECT_ID) output.push(value);
    for (const child of Object.values(value)) collectEffectObjects(child, output);
  }
  return output;
}

function models() {
  const placements = [
    "generic-damage-additive-zone-before-critical-lucky-and-mitigation",
    "generic-damage-additive-zone-after-critical-lucky-before-mitigation",
    "generic-damage-additive-zone-after-mitigation-before-packet-final",
    "independent-multiplicative-stage-before-critical-lucky-and-mitigation",
    "independent-multiplicative-stage-after-critical-lucky-before-mitigation",
    "terminal-multiplicative-stage-after-all-other-factors",
  ];
  const roundings = ["floor", "positive-round-half-up", "ceil"];
  return placements.flatMap((placement) => roundings.map((rounding) => ({
    model_id: `${placement}:${rounding}`,
    placement,
    rounding,
    exact_controlled_rows_tested: 0,
    exactly_conserving_controlled_rows: 0,
    rejected_controlled_rows: 0,
    disposition: "unresolved-no-exact-absent-baseline",
    accepted: false,
  })));
}

function build(parsed) {
  const files = {
    correlation: required(parsed, "correlation"),
    routing: required(parsed, "routing"),
    counterfactual: required(parsed, "counterfactual"),
  };
  const correlation = readJson(files.correlation, "correlation receipt");
  const routing = readJson(files.routing, "routing receipt");
  const counterfactual = readJson(files.counterfactual, "counterfactual receipt");

  assert(String(counterfactual.game_build) === BUILD, "counterfactual build mismatch");
  const route = only((routing.effect_routes ?? []).filter((row) => Number(row.effect_id) === EFFECT_ID),
    `routing row for effect ${EFFECT_ID}`);
  const vulnerability = only(route.components.filter((row) => row.component_key === "target-vulnerability"),
    "target-vulnerability component");
  const attackReduction = only(route.components.filter((row) => row.component_key === "attack-stat-reduction"),
    "attack-stat-reduction component");
  const terminal = only(collectEffectObjects(correlation).filter((row) =>
    Object.hasOwn(row, "recipient_window_damage_events") &&
    Object.hasOwn(row, "target_window_damage_events")), "terminal correlation row");
  const effect = only((counterfactual.effects ?? []).filter((row) =>
    row.locus === "target" && Number(row.effect_id) === EFFECT_ID), "target counterfactual effect");
  const near = only((counterfactual.near_controlled_target_diagnostic ?? []).filter((row) =>
    Number(row.effect_id) === EFFECT_ID), "near-controlled diagnostic");

  assert(vulnerability.contribution_scope === "targeted", "vulnerability scope is not targeted");
  assert(attackReduction.contribution_scope === "effect-recipient", "attack reduction scope changed");
  assert(attackReduction.formula_replay_status === "not-outgoing-rdps", "attack reduction disposition changed");
  assert(Number(terminal.recipient_window_damage_events) === 7_318, "expected 7,318 source/outgoing rows");
  assert(Number(terminal.target_window_damage_events) === 488_985, "expected 488,985 target/incoming rows");
  assert(Number(effect.observation.observed_samples) === 143_573, "target-active formula sample count changed");
  assert(Number(effect.exact_recorded_inputs.controlled_groups) === 0, "exact controls are no longer zero");

  const modelRows = models();
  const report = {
    schema_version: 1,
    generated_by: "bpsr-oblivion-dream-component-lane-discriminant",
    game_build: BUILD,
    effect_id: EFFECT_ID,
    label: vulnerability.label,
    inputs: Object.fromEntries(Object.entries(files).map(([key, file]) => [key, descriptor(file)])),
    memory_policy: {
      maximum_allowed_mib: 8_192,
      large_formula_cohort_reopened: false,
      receipt_only_inputs: true,
    },
    component_lane_adjudication: {
      requested_row_count: 7_318,
      requested_rows_are_vulnerability_beneficiary_rows: false,
      requested_rows_actual_lane: "effect-recipient-as-damage-source/outgoing",
      requested_rows_component: attackReduction.label,
      requested_rows_component_transfer_eligibility: attackReduction.transfer_eligibility,
      requested_rows_component_formula_replay_status: attackReduction.formula_replay_status,
      requested_rows_damage: terminal.recipient_window_damage,
      correct_vulnerability_lane: "effect-recipient-as-damage-target/incoming",
      correct_vulnerability_aggregate_rows: Number(terminal.target_window_damage_events),
      correct_vulnerability_aggregate_damage: terminal.target_window_damage,
      formula_cohort_target_active_rows: Number(effect.observation.observed_samples),
      formula_cohort_target_active_groups: Number(effect.exact_recorded_inputs.present_groups),
    },
    exhaustive_exact_discriminant: {
      candidate_magnitude_basis_points: 1_000,
      candidate_placements: 6,
      candidate_integer_roundings: 3,
      candidate_models: modelRows.length,
      models: modelRows,
      exact_present_groups: Number(effect.exact_recorded_inputs.present_groups),
      exact_present_samples: Number(effect.exact_recorded_inputs.present_samples),
      exact_absent_status_state_unobserved_groups:
        Number(effect.exact_recorded_inputs.absent_status_state_unobserved_groups),
      exact_absent_identity_group_unobserved_groups:
        Number(effect.exact_recorded_inputs.absent_identity_group_unobserved_groups),
      exact_controlled_groups: Number(effect.exact_recorded_inputs.controlled_groups),
      exact_sample_comparisons: Number(effect.exact_recorded_inputs.sample_comparisons),
      uniquely_selected_model: null,
      accepted_model_count: 0,
      unresolved_model_count: modelRows.length,
      exact_conservation_proven: false,
      explanation: "Packet-final values with the candidate present do not determine the absent subtotal, the active composite generic-damage factor, operation order, or an integer boundary. The receipt contains no otherwise-identical effect-absent group, so none of the enumerated models can be accepted or rejected by exact conservation.",
    },
    nearest_existing_discriminator: {
      candidate_absent_near_pairs: Number(near.candidate_absent_near_pairs),
      divergent_output_pairs: Number(near.divergent_output_pairs),
      minimum_transition_distance: near.minimum_transition_distance,
      bounded_transition_signatures: [...new Set((near.variants ?? []).flatMap((variant) =>
        (variant.examples ?? []).map((example) => JSON.stringify({
          target_attribute_ids: (example.target_attribute_transitions_excluding_current_hp ?? [])
            .map((row) => Number(row.attribute_id)).sort((a, b) => a - b),
          target_present_only_effect_ids: (example.target_status_present_only_co_transitions ?? [])
            .map((row) => Number(row.effect_id)).sort((a, b) => a - b),
          target_absent_only_effect_ids: (example.target_status_absent_only_co_transitions ?? [])
            .map((row) => Number(row.effect_id)).sort((a, b) => a - b),
          ability_id: Number(example.ability_id),
          outputs_equal: Boolean(example.outputs_equal),
        })))).values()].map((row) => JSON.parse(row)),
      smallest_safe_closure: [
        "Produce one otherwise-identical current-build packet-final damage identity with effect 3003012 target-active and target-absent; every source/target attribute, every other source/target status, every referenced provider attribute state, and packet formula input must match.",
        "Alternatively prove the exact current-build genericDamage composite, the 1000-basis-point additive share, its placement relative to critical/lucky/mitigation, and every integer boundary from an independent executable or server-handler authority.",
      ],
    },
    promotion: {
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      reason: "wrong requested lane and zero exact controlled vulnerability comparisons",
    },
  };

  const output = required(parsed, "output");
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`wrote ${output}`);
}

function verify(parsed) {
  const report = readJson(required(parsed, "input"), "discriminant report");
  assert(report.schema_version === 1, "schema mismatch");
  assert(report.game_build === BUILD && Number(report.effect_id) === EFFECT_ID, "identity mismatch");
  assert(report.component_lane_adjudication.requested_rows_are_vulnerability_beneficiary_rows === false,
    "wrong-lane adjudication missing");
  assert(report.component_lane_adjudication.correct_vulnerability_aggregate_rows === 488_985,
    "correct target lane count mismatch");
  assert(report.exhaustive_exact_discriminant.candidate_models === 18, "model count mismatch");
  assert(report.exhaustive_exact_discriminant.exact_controlled_groups === 0, "controlled groups changed");
  assert(report.exhaustive_exact_discriminant.uniquely_selected_model === null, "model was invented");
  assert(report.promotion.provider_rdps_credit_allowed === false &&
    report.promotion.runtime_promotion_allowed === false, "promotion must remain fail-closed");
  console.log(`verified ${path.resolve(parsed.input)}`);
}

const [command, ...rest] = process.argv.slice(2);
const parsed = parseArgs(rest);
if (command === "build") build(parsed);
else if (command === "verify") verify(parsed);
else throw new Error("usage: node tools/bpsr-oblivion-dream-component-lane-discriminant.mjs build --correlation <json> --routing <json> --counterfactual <json> --output <json> | verify --input <json>");
