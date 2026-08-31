#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 3;
const GENERATED_BY = "tools/bpsr-fatal-spiral-controlled-pair-worklist.mjs";
const GAME_BUILD = "24687926";
const EFFECT_ID = 2110125;
const PROVIDER_EFFECT_ID = 2110124;
const FAMILY = [13100, 13101, 13102, 13103, 13104, 13105];

function fail(message) {
  throw new Error(message);
}

function readJson(file, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    fail(`Cannot read ${label} ${file}: ${error.message}`);
  }
}

function descriptor(file) {
  const bytes = fs.readFileSync(file);
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(canonical(copy)).digest("hex").toUpperCase();
}

function ceilDiv(numerator, denominator) {
  if (numerator < 0n || denominator <= 0n) fail("ceilDiv expects a nonnegative numerator and positive denominator");
  return (numerator + denominator - 1n) / denominator;
}

function preimageInterval(output, factor, denominator, rounding) {
  const y = BigInt(output);
  const f = BigInt(factor);
  const d = BigInt(denominator);
  if (y < 0n || f <= 0n || d <= 0n) fail("Invalid nonnegative fixed-point interval input");
  if (rounding === "floor") {
    return {
      minimum: ceilDiv(y * d, f),
      maximum: ceilDiv((y + 1n) * d, f) - 1n,
    };
  }
  if (rounding === "nearest-half-up") {
    const lowerNumerator = 2n * y * d > d ? 2n * y * d - d : 0n;
    const upperNumerator = 2n * (y + 1n) * d - d;
    return {
      minimum: ceilDiv(lowerNumerator, 2n * f),
      maximum: ceilDiv(upperNumerator, 2n * f) - 1n,
    };
  }
  fail(`Unsupported rounding ${rounding}`);
}

function intersection(left, right) {
  const minimum = left.minimum > right.minimum ? left.minimum : right.minimum;
  const maximum = left.maximum < right.maximum ? left.maximum : right.maximum;
  return minimum <= maximum ? { minimum, maximum } : null;
}

function validateDamageFrontier(frontier) {
  const closure = frontier.proof_closure ?? {};
  if (
    frontier.schema_version !== 2 ||
    frontier.generated_by !== "tools/bpsr-fatal-spiral-damage-stage-frontier.mjs" ||
    frontier.game_build !== GAME_BUILD ||
    Number(frontier.identity?.effect_id) !== EFFECT_ID ||
    Number(frontier.identity?.provider_marker_effect_id) !== PROVIDER_EFFECT_ID ||
    JSON.stringify(frontier.identity?.generic_element_attribute_family) !== JSON.stringify(FAMILY) ||
    closure.exact_event_time_provider_tier_join_complete !== true ||
    closure.exact_source_side_effect_recipient_to_damage_actor_join_complete !== true ||
    closure.audited_damage_membership_selection_conserved !== true ||
    closure.exact_build_client_consumer_search_exhausted !== true ||
    closure.controlled_pair_search_exhausted_for_retained_cohort !== true ||
    closure.combat_damage_stage_consumer_proven !== false ||
    closure.exact_operation_order_proven !== false ||
    closure.exact_integer_rounding_proven !== false ||
    closure.provider_rdps_credit_allowed !== false
  ) fail("Fatal Spiral damage-stage frontier is unsafe or incompatible");
}

function validateConsumerFrontier(frontier) {
  if (
    frontier.schema_version !== 3 ||
    frontier.generated_by !== "tools/bpsr-all-element-damage-consumer-frontier.mjs" ||
    frontier.game_build !== GAME_BUILD ||
    JSON.stringify(frontier.identity?.attribute_family) !== JSON.stringify(FAMILY) ||
    frontier.proof_closure?.server_damage_operator_present_in_reviewed_client_static_inventory !== false ||
    frontier.proof_closure?.executable_all_element_damage_consumer_proven !== false ||
    frontier.proof_closure?.exact_native_immediate_family_search_exhausted !== true ||
    frontier.proof_closure?.combat_relevant_exact_family_immediate_consumer_found !== false ||
    frontier.proof_closure?.exact_build_generic_instantiation_indexed !== true ||
    frontier.proof_closure?.bounded_direct_getter_call_search_exhausted !== true ||
    frontier.proof_closure?.combat_relevant_literal_attribute_getter_consumer_found !== false ||
    frontier.proof_closure?.exact_method_pointer_slot_inventory_complete !== true ||
    frontier.proof_closure?.exact_rip_relative_slot_reference_search_exhausted !== true ||
    frontier.proof_closure?.indexed_metadata_dispatch_or_protected_consumer_excluded !== false ||
    frontier.proof_closure?.computed_indirect_table_driven_or_protected_consumer_excluded !== false ||
    frontier.proof_closure?.provider_rdps_credit_allowed !== false ||
    frontier.acquisition_frontier?.structurally_absent_remote_cast_packets_required !== false
  ) fail("All-element consumer frontier is unsafe or incompatible");
}

function validateFamilyProof(proof) {
  if (
    proof.schema_version !== 1 ||
    proof.generated_by !== "tools/bpsr-all-element-fixed-point-family-proof.mjs" ||
    proof.game_build !== GAME_BUILD ||
    Number(proof.identity?.effect_id) !== EFFECT_ID ||
    Number(proof.identity?.provider_marker_effect_id) !== PROVIDER_EFFECT_ID ||
    Number(proof.fixed_point_family?.denominator) !== 10000 ||
    JSON.stringify(proof.provider_scalar?.tier_basis_points) !== JSON.stringify([600, 700, 800, 900, 1000]) ||
    Number(proof.provider_scalar?.packet_attribute_oracle?.tier) !== 5 ||
    Number(proof.provider_scalar?.packet_attribute_oracle?.baseline_value) !== 316 ||
    Number(proof.provider_scalar?.packet_attribute_oracle?.applied_value) !== 1316 ||
    Number(proof.provider_scalar?.packet_attribute_oracle?.applied_delta) !== 1000 ||
    proof.policy?.runtime_transfer_enabled !== false
  ) fail("All-element family proof is unsafe or incompatible");
}

function build(options) {
  const damageFile = path.resolve(options.damageStageFrontier);
  const consumerFile = path.resolve(options.consumerFrontier);
  const familyFile = path.resolve(options.familyProof);
  const damage = readJson(damageFile, "Fatal Spiral damage-stage frontier");
  const consumer = readJson(consumerFile, "all-element consumer frontier");
  const family = readJson(familyFile, "all-element family proof");
  validateDamageFrontier(damage);
  validateConsumerFrontier(consumer);
  validateFamilyProof(family);

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game: "blue-protocol-star-resonance",
    game_build: GAME_BUILD,
    identity: {
      imagine_skill_id: 3957,
      provider_marker_effect_id: PROVIDER_EFFECT_ID,
      effect_id: EFFECT_ID,
      provider_owned_direct_damage_exclusion_ids: [111007400108],
      attribute_family: FAMILY,
      fixed_point_denominator: 10000,
    },
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      source_side_join: "effect endpoint equals damage actor",
      target_allegiance_assumed: false,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      remote_player_cast_packets_required: false,
      missing_remote_cast_packets_are_zero: false,
      current_character_snapshots_may_replace_historical_state: false,
      one_changed_axis_required: true,
      compatibility_with_a_candidate_is_formula_authority: false,
      unresolved_pairs_are_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      fatal_spiral_damage_stage_frontier: descriptor(damageFile),
      all_element_damage_consumer_frontier: descriptor(consumerFile),
      all_element_fixed_point_family_proof: descriptor(familyFile),
    },
    current_evidence: {
      retained_formula_samples: damage.reviewed_evidence.source_formula_cohort.samples,
      audited_damage_event_memberships:
        damage.reviewed_evidence.gap_bounded_source_lifecycles.audited_damage_event_memberships,
      controlled_pairs_available: damage.reviewed_evidence.counterfactual_exhaustion.exact_controlled_groups,
      observed_baseline_attribute_value: 316,
      observed_tier_5_attribute_value: 1316,
      observed_tier_5_delta_basis_points: 1000,
      executable_damage_consumer_proven: false,
      exact_native_immediate_family_search_exhausted: true,
      combat_relevant_exact_family_immediate_consumer_found: false,
      bounded_direct_getter_call_search_exhausted: true,
      combat_relevant_literal_attribute_getter_consumer_found: false,
      exact_method_pointer_slot_inventory_complete: true,
      exact_rip_relative_slot_reference_search_exhausted: true,
      indexed_metadata_dispatch_or_protected_consumer_excluded: false,
      computed_indirect_table_driven_or_protected_consumer_excluded: false,
    },
    primary_capture_variant: {
      provider_tier: 5,
      absent: {
        effect_2110125_active_on_damage_actor: false,
        expected_attribute_13100: 316,
      },
      present: {
        effect_2110125_active_on_damage_actor: true,
        expected_attribute_13100: 1316,
        exact_provider_and_tier_required: true,
      },
      expected_attribute_delta: {
        attribute_ids: [13100, 13101, 13102],
        delta_each: 1000,
        attribute_ids_required_unchanged: [13103, 13104, 13105],
      },
      lower_tiers: "diagnostic until the exact event-time packet transition is observed for that tier",
    },
    controlled_pair_contract: {
      capture_scope: [
        "same build 24687926 and byte-identical protocol pack",
        "same session, run, scene, damage actor, and target entity",
        "both events outside every decode, TCP, capture-drop, and unknown-route gap",
        "complete event-time source, target, and status-provider attribute snapshots",
      ],
      invariant_damage_identity_fields: [
        "ability_id", "hit_event_id", "damage_source", "damage_type", "packet property",
        "owner_level", "owner_stage", "type_flags", "normal_hit", "reported_critical",
        "lucky", "passive_uuid", "rainbow", "damage_mode", "skill_effect_uuid",
        "skill_effect_group_index", "skill_effect_component_index",
        "skill_effect_component_count", "hit_parts", "damage_weight",
      ],
      invariant_state_fields: [
        "source actor identity and all non-13100..13105 fight attributes",
        "source status multiset after removing only effect 2110125",
        "target actor identity, complete fight attributes, and complete status multiset",
        "all status-provider identities and their event-time attribute states",
        "target HP and shield pre-state unless an independently proven formula declares them irrelevant",
      ],
      only_allowed_differences: [
        "effect 2110125 lifecycle presence on the damage actor",
        "its exact provider and tier evidence",
        "the proven tier-5 +1000 transition on attributes 13100, 13101, and 13102",
      ],
      damage_event_requirements: [
        "positive ordinary damage to HP with no shield absorption",
        "exact numeric packet property in 1 through 8",
        "normal non-critical non-lucky hit for the primary discriminator",
        "not provider-owned direct damage action 111007400108",
        "exactly one exact-build damage surface row and authoritative owner-stage selection",
      ],
      rejection_rules: [
        "any other source attribute or status difference",
        "any target attribute, status, HP, shield, actor, or scene difference",
        "any packet calculation identity or damage-surface ambiguity",
        "any data-quality boundary intersecting either lifecycle or damage event",
        "any missing value treated as equal, false, or zero",
      ],
      replication_requirement:
        "retain at least two independently repeated qualifying pairs; output nondeterminism keeps every pair diagnostic",
    },
    exact_integer_discriminator: {
      scope: "tests only the hypothesis that the all-element factor is the final fixed-point multiplier on one nonnegative integer subtotal",
      baseline_factor: 10316,
      tier_5_present_factor: 11316,
      denominator: 10000,
      candidates: [
        {
          name: "final-all-element-floor",
          equation: "damage = floor(integer_subtotal * all_element_factor / 10000)",
          exact_preimage_interval:
            "ceil(damage*10000/factor) through ceil((damage+1)*10000/factor)-1",
        },
        {
          name: "final-all-element-nearest-half-up",
          equation: "damage = floor((2*integer_subtotal*all_element_factor + 10000) / 20000)",
          exact_preimage_interval:
            "ceil(max(0,2*damage*10000-10000)/(2*factor)) through ceil((2*(damage+1)*10000-10000)/(2*factor))-1",
        },
      ],
      pair_test:
        "intersect the absent and present exact-preimage intervals; an empty intersection rejects that candidate",
      authority_rule:
        "compatibility does not prove stage identity; replicated pairs must select one candidate, reject alternatives, bind the subtotal to a proven damage stage, and conserve packet integers",
    },
    proof_closure: {
      exact_capture_contract_defined: true,
      exact_integer_candidate_discriminator_defined: true,
      current_controlled_pairs_available: false,
      combat_damage_stage_consumer_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      multi_provider_stacking_and_split_proven: false,
      integer_counterfactual_projection_complete: false,
      conservation_replay_complete: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  verify(report);
  return report;
}

function verify(report) {
  const closure = report.proof_closure ?? {};
  if (
    report.schema_version !== SCHEMA_VERSION || report.generated_by !== GENERATED_BY ||
    report.game_build !== GAME_BUILD || Number(report.identity?.effect_id) !== EFFECT_ID ||
    JSON.stringify(report.identity?.attribute_family) !== JSON.stringify(FAMILY) ||
    report.topology?.target_allegiance_assumed !== false ||
    report.policy?.remote_player_cast_packets_required !== false ||
    report.policy?.missing_remote_cast_packets_are_zero !== false ||
    report.policy?.current_character_snapshots_may_replace_historical_state !== false ||
    report.policy?.compatibility_with_a_candidate_is_formula_authority !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    Number(report.current_evidence?.controlled_pairs_available) !== 0 ||
    report.current_evidence?.bounded_direct_getter_call_search_exhausted !== true ||
    report.current_evidence?.combat_relevant_literal_attribute_getter_consumer_found !== false ||
    report.current_evidence?.exact_method_pointer_slot_inventory_complete !== true ||
    report.current_evidence?.exact_rip_relative_slot_reference_search_exhausted !== true ||
    report.current_evidence?.indexed_metadata_dispatch_or_protected_consumer_excluded !== false ||
    Number(report.primary_capture_variant?.absent?.expected_attribute_13100) !== 316 ||
    Number(report.primary_capture_variant?.present?.expected_attribute_13100) !== 1316 ||
    Number(report.exact_integer_discriminator?.baseline_factor) !== 10316 ||
    Number(report.exact_integer_discriminator?.tier_5_present_factor) !== 11316 ||
    closure.exact_capture_contract_defined !== true ||
    closure.exact_integer_candidate_discriminator_defined !== true ||
    closure.current_controlled_pairs_available !== false ||
    closure.combat_damage_stage_consumer_proven !== false ||
    closure.exact_operation_order_proven !== false ||
    closure.exact_integer_rounding_proven !== false ||
    closure.formula_authority !== false || closure.runtime_authority !== false ||
    closure.ui_display_authority !== false || closure.provider_rdps_credit_allowed !== false ||
    report.content_sha256 !== digest(report)
  ) fail("Fatal Spiral controlled-pair worklist is unsafe or has an invalid digest");
}

function parse(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!flag?.startsWith("--") || value == null) fail(`Invalid argument ${flag ?? "<missing>"}`);
    args[flag.slice(2)] = value;
  }
  return args;
}

function required(args, name) {
  if (!args[name]) fail(`Missing --${name}`);
  return args[name];
}

function selfTest() {
  const subtotal = 100000n;
  for (const rounding of ["floor", "nearest-half-up"]) {
    const apply = (factor) => rounding === "floor"
      ? subtotal * BigInt(factor) / 10000n
      : (2n * subtotal * BigInt(factor) + 10000n) / 20000n;
    const absent = preimageInterval(apply(10316), 10316, 10000, rounding);
    const present = preimageInterval(apply(11316), 11316, 10000, rounding);
    const common = intersection(absent, present);
    if (!common || common.minimum > subtotal || common.maximum < subtotal) {
      fail(`${rounding} interval solver lost the generating subtotal`);
    }
  }
  const impossible = intersection(
    preimageInterval(1, 10316, 10000, "floor"),
    preimageInterval(100, 11316, 10000, "floor"),
  );
  if (impossible !== null) fail("interval solver accepted an impossible pair");
  console.log("bpsr-fatal-spiral-controlled-pair-worklist self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(path.resolve(required(args, "input")), "controlled-pair worklist"));
    console.log("Fatal Spiral controlled-pair worklist verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = path.resolve(required(args, "output"));
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = build({
      damageStageFrontier: required(args, "damage-stage-frontier"),
      consumerFrontier: required(args, "consumer-frontier"),
      familyProof: required(args, "family-proof"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, proof_closure: report.proof_closure }, null, 2));
  } else {
    console.log("Usage:\n  node tools/bpsr-fatal-spiral-controlled-pair-worklist.mjs build --damage-stage-frontier <json> --consumer-frontier <json> --family-proof <json> --output <json>\n  node tools/bpsr-fatal-spiral-controlled-pair-worklist.mjs verify --input <json>\n  node tools/bpsr-fatal-spiral-controlled-pair-worklist.mjs self-test");
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
