#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GAME_BUILD = "24687926";
const EFFECT_ID = 3_003_052;
const SCHEMA_VERSION = 2;
const SUPPORTED_SCHEMA_VERSIONS = new Set([1, 2]);
const SELECTED_SEQUENCE = 36_480;
const PHYSICAL_DEFENSE_ATTRIBUTE_ID = 11_350;
const EXTERNAL_COMMIT = "e21e06c07559396d4432c2541319c7c08e5caf31";
const DEFAULT_RESEARCH_ROOT = path.join(
  "plugins",
  "games",
  "blue-protocol-star-resonance",
  "research",
  "game-file-inventory",
  "global",
  "steam-24687926",
);

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function parseArgs(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value === undefined) {
      throw new Error(`unknown or incomplete argument: ${flag ?? "<missing>"}`);
    }
    values.set(flag.slice(2), value);
  }
  return values;
}

function required(values, key) {
  const value = values.get(key);
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function optional(values, key, fallback) {
  return values.get(key) ?? fallback;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function receipt(filePath) {
  const absolutePath = path.resolve(filePath);
  const bytes = fs.readFileSync(absolutePath);
  return {
    path: absolutePath.replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function gcd(left, right) {
  let a = left < 0n ? -left : left;
  let b = right < 0n ? -right : right;
  while (b !== 0n) [a, b] = [b, a % b];
  return a;
}

function addRational(left, right) {
  const common = gcd(left.denominator, right.denominator);
  const leftScale = right.denominator / common;
  const rightScale = left.denominator / common;
  const numerator = left.numerator * leftScale + right.numerator * rightScale;
  const denominator = left.denominator * leftScale;
  const divisor = gcd(numerator, denominator);
  return {
    numerator: numerator / divisor,
    denominator: denominator / divisor,
  };
}

function floorRational(value) {
  return value.numerator / value.denominator;
}

function ceilRational(value) {
  return (value.numerator + value.denominator - 1n) / value.denominator;
}

function nearestHalfUp(value) {
  return (2n * value.numerator + value.denominator) / (2n * value.denominator);
}

function rationalFromTrace(row) {
  const numerator = BigInt(row.contribution.numerator);
  const denominator = BigInt(row.contribution.denominator);
  assert(numerator >= 0n && denominator > 0n);
  return { numerator, denominator };
}

function formulaStateKey(sample) {
  return JSON.stringify([
    sample.session_id,
    sample.run_ordinal,
    sample.scene_id,
    sample.source_entity_uuid,
    sample.direct_source_entity_uuid,
    sample.target_entity_uuid,
    sample.source_attribute_state_id,
    sample.target_attribute_state_id,
    sample.source_status_state_id,
    sample.target_status_state_id,
    sample.critical,
    sample.lucky,
    sample.packet?.damage_mode,
    sample.packet?.owner_level,
    sample.packet?.owner_stage,
    sample.packet?.type_flags,
    sample.packet?.property,
    sample.packet?.normal_hit,
    sample.packet?.passive_uuid,
    sample.packet?.rainbow,
  ]);
}

function contentHash(report) {
  const clone = structuredClone(report);
  delete clone.content_sha256;
  return crypto.createHash("sha256").update(JSON.stringify(clone)).digest("hex");
}

function validateInputs(
  formula,
  stages,
  trace,
  acquisition,
  cohort,
  crossCoefficientCohort,
  replicationTrace,
  preflight,
  pack,
) {
  assert.equal(String(formula.game_build), GAME_BUILD);
  assert.equal(formula.external_source?.commit, EXTERNAL_COMMIT);
  assert.equal(formula.policy?.external_calculator_is_discovery_evidence_only, true);
  assert.equal(stages.policy?.runtime_formula_authority, false);
  assert.ok(Array.isArray(stages.proven_stages));
  assert.ok(Array.isArray(stages.unresolved_stages));
  assert.equal(String(trace.game_build), GAME_BUILD);
  assert.equal(Number(trace.effect_id), EFFECT_ID);
  assert.equal(trace.policy?.runtime_promotion_allowed, false);
  assert.ok(Array.isArray(trace.traces) && trace.traces.length > 0);
  if (replicationTrace) {
    assert.equal(String(replicationTrace.game_build), GAME_BUILD);
    assert.equal(Number(replicationTrace.effect_id), EFFECT_ID);
    assert.equal(replicationTrace.policy?.runtime_promotion_allowed, false);
    assert.ok(Array.isArray(replicationTrace.traces) && replicationTrace.traces.length > 0);
    assert.notEqual(replicationTrace.session_id, trace.session_id);
  }
  assert.equal(String(acquisition.game_build), GAME_BUILD);
  assert.equal(Number(acquisition.effect_id), EFFECT_ID);
  assert.equal(acquisition.conclusion?.exact_final_server_integer_counterfactual_proven, false);
  assert.equal(String(cohort.game_build), GAME_BUILD);
  assert.ok(Array.isArray(cohort.samples) && Array.isArray(cohort.attribute_states));
  assert.equal(String(crossCoefficientCohort.game_build), GAME_BUILD);
  assert.ok(
    Array.isArray(crossCoefficientCohort.samples) &&
      Array.isArray(crossCoefficientCohort.attribute_states),
  );
  assert.equal(String(preflight.game_build), GAME_BUILD);
  assert.equal(preflight.ready_for_snapshot, true);
  assert.equal(preflight.runtime_promotion_allowed, false);
  assert.equal(String(pack.target?.build_id), GAME_BUILD);
}

function generate(values) {
  const formulaPath = path.resolve(optional(
    values,
    "formula-hypothesis",
    path.join(DEFAULT_RESEARCH_ROOT, "external-damage-formula-hypothesis.v1.json"),
  ));
  const stagesPath = path.resolve(optional(
    values,
    "damage-stages",
    path.join(
      "plugins",
      "games",
      "blue-protocol-star-resonance",
      "game-data",
      "catalog",
      "formulas",
      "damage-formula-stages.current-build.v1.json",
    ),
  ));
  const preflightPath = path.resolve(optional(
    values,
    "preflight",
    path.join(DEFAULT_RESEARCH_ROOT, "rdps-build-preflight.v3.json"),
  ));
  const packPath = path.resolve(optional(
    values,
    "pack",
    path.join(
      "plugins",
      "games",
      "blue-protocol-star-resonance",
      "protocol-packs",
      "global",
      "steam-24687926",
      "pack.json",
    ),
  ));
  const tracePath = path.resolve(required(values, "trace"));
  const replicationTracePath = values.get("replication-trace")
    ? path.resolve(values.get("replication-trace"))
    : null;
  const acquisitionPath = path.resolve(required(values, "integer-acquisition"));
  const cohortPath = path.resolve(required(values, "formula-cohort"));
  const crossCoefficientCohortPath = path.resolve(
    required(values, "cross-coefficient-cohort"),
  );
  const outputPath = path.resolve(optional(
    values,
    "output",
    path.join(DEFAULT_RESEARCH_ROOT, "harmony-grace-3003052.calculator-stage-adjudication.v1.json"),
  ));

  const rssSamples = [{ stage: "start", rss_bytes: process.memoryUsage().rss }];
  const formula = readJson(formulaPath);
  const stages = readJson(stagesPath);
  const trace = readJson(tracePath);
  const replicationTrace = replicationTracePath ? readJson(replicationTracePath) : null;
  const acquisition = readJson(acquisitionPath);
  const cohort = readJson(cohortPath);
  const crossCoefficientCohort = readJson(crossCoefficientCohortPath);
  const preflight = readJson(preflightPath);
  const pack = readJson(packPath);
  rssSamples.push({ stage: "inputs_loaded", rss_bytes: process.memoryUsage().rss });
  validateInputs(
    formula,
    stages,
    trace,
    acquisition,
    cohort,
    crossCoefficientCohort,
    replicationTrace,
    preflight,
    pack,
  );

  let exactTotal = { numerator: 0n, denominator: 1n };
  let perRowFloor = 0n;
  let perRowCeil = 0n;
  let perRowNearest = 0n;
  for (const row of trace.traces) {
    const value = rationalFromTrace(row);
    exactTotal = addRational(exactTotal, value);
    perRowFloor += floorRational(value);
    perRowCeil += ceilRational(value);
    perRowNearest += nearestHalfUp(value);
  }
  assert.equal(exactTotal.numerator.toString(), trace.summary.exact_contribution.numerator);
  assert.equal(exactTotal.denominator.toString(), trace.summary.exact_contribution.denominator);

  const traceReplications = [trace, ...(replicationTrace ? [replicationTrace] : [])].map(
    summarizeTraceEquation,
  );
  let replicatedExactTotal = { numerator: 0n, denominator: 1n };
  for (const entry of traceReplications) {
    replicatedExactTotal = addRational(replicatedExactTotal, {
      numerator: BigInt(entry.exact_contribution.numerator),
      denominator: BigInt(entry.exact_contribution.denominator),
    });
  }

  const sample = cohort.samples.find((row) => Number(row.sequence) === SELECTED_SEQUENCE);
  assert(sample, `selected sequence ${SELECTED_SEQUENCE} is absent from formula cohort`);
  const targetAttributes = cohort.attribute_states[Number(sample.target_attribute_state_id)];
  assert(Array.isArray(targetAttributes));
  const targetAttributeIds = targetAttributes.map((row) => Number(row.attribute_id)).sort((a, b) => a - b);
  const targetPhysicalDefense = targetAttributes.find(
    (row) => Number(row.attribute_id) === PHYSICAL_DEFENSE_ATTRIBUTE_ID,
  );
  const exactStateGroups = new Map();
  for (const row of crossCoefficientCohort.samples) {
    const key = formulaStateKey(row);
    const group = exactStateGroups.get(key) ?? new Set();
    group.add(Number(row.hit_event_id));
    exactStateGroups.set(key, group);
  }
  const crossHitExactStateGroups = [...exactStateGroups.values()].filter(
    (hitIds) => hitIds.size > 1,
  ).length;

  const classStage = formula.hypothesis_stages.find(
    (row) => row.stage === "class_main_stat_to_attack",
  );
  const coefficientStage = formula.hypothesis_stages.find(
    (row) => row.stage === "coefficient_and_fixed",
  );
  const multiplierStage = formula.hypothesis_stages.find(
    (row) => row.stage === "multiplicative_buckets",
  );
  const selectedWorkbench = formula.selected_hit_workbench;
  assert(classStage && coefficientStage && multiplierStage);
  assert.equal(Number(selectedWorkbench?.sample_sequence), SELECTED_SEQUENCE);

  const perRowValues = {
    floor_each_row: perRowFloor,
    ceil_each_row: perRowCeil,
    nearest_half_up_each_row: perRowNearest,
  };
  const distinctPerRowTotals = new Set(
    Object.values(perRowValues).map((value) => value.toString()),
  );
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-harmony-calculator-stage-adjudication.mjs",
    game_build: GAME_BUILD,
    effect_id: EFFECT_ID,
    purpose: "use the pinned Season-3 damage calculator as a stage hypothesis while independently adjudicating every current-build runtime boundary",
    policy: {
      numeric_effect_ids_and_build_identity_are_authoritative: true,
      external_calculator_is_discovery_evidence_only: true,
      copied_external_code_is_runtime_authority: false,
      missing_target_stats_may_be_invented_or_backfilled: false,
      unresolved_integer_boundaries_are_preserved: true,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
    },
    inputs: {
      external_damage_formula_hypothesis: receipt(formulaPath),
      damage_stage_boundary_catalog: {
        ...receipt(stagesPath),
        declared_client_build: String(stages.client_build),
        authority: "historical and packet-stage boundary inventory; exact-build claims are separately required",
      },
      corrected_harmony_trace: receipt(tracePath),
      ...(replicationTracePath
        ? { independent_replication_trace: receipt(replicationTracePath) }
        : {}),
      final_integer_acquisition: receipt(acquisitionPath),
      selected_hit_formula_cohort: receipt(cohortPath),
      cross_coefficient_formula_cohort: receipt(crossCoefficientCohortPath),
      current_build_preflight: receipt(preflightPath),
      current_build_protocol_pack: receipt(packPath),
    },
    external_source: {
      webpage: formula.external_source.webpage,
      repository: formula.external_source.repository,
      commit: EXTERNAL_COMMIT,
      source_hashes: formula.external_source.source_files
        .filter((row) => row.sha256)
        .map((row) => ({ path: row.path, sha256: row.sha256 })),
      independently_expressed_stage_hypotheses: {
        class_main_stat_to_attack: classStage.candidate_expression,
        coefficient_and_fixed: coefficientStage.candidate_expression,
        multiplicative_buckets: multiplierStage.candidate_expression,
      },
      authority_boundary: "the calculator retains floating-point damage through its final skill result and explicitly marks final-after-other ordering as an assumption; it cannot select the game's final integer boundary",
    },
    stage_adjudication: [
      {
        stage: "recipient_primary_percent",
        effect_magnitude: "+200 raw basis points from effect 3003052",
        current_build_state: "exact provider-owned lifecycle packet transition",
        closed_for_selected_scope: true,
      },
      {
        stage: "class_11_primary_to_attack",
        candidate_expression: "floor(primary * 58 / 100)",
        current_build_state: "58/100 agrees with 15 exact packet boundaries; the former 1/8 complete route agrees with none",
        closed_for_selected_scope: true,
      },
      {
        stage: "attack_family",
        current_build_state: "exact packet family reversal is retained per accepted damage row",
        closed_for_selected_scope: true,
      },
      {
        stage: "coefficient_and_fixed",
        current_build_state: "each accepted row carries exact numeric ability, hit, coefficient, fixed term, and active/without-provider stage bodies",
        closed_for_selected_scope: true,
      },
      {
        stage: "target_mitigation_and_multiplier_buckets",
        calculator_hypothesis: "downstream multiplicative buckets cancel in the active-stage share only if there is no additive term or intervening integer boundary",
        current_build_state: "bucket purity, order, and intervening integer boundaries remain unproven",
        closed_for_selected_scope: false,
      },
      {
        stage: "final_server_integer",
        candidates: ["floor", "ceil", "nearest_half_up"],
        current_build_state: "no replicated exact A/B/A group selects one boundary across two stage signatures",
        closed_for_selected_scope: false,
      },
    ],
    selected_hit_target_boundary: {
      sample_sequence: SELECTED_SEQUENCE,
      ability_id: Number(sample.ability_id),
      hit_event_id: Number(sample.hit_event_id),
      target_numeric_monster_id: Number(sample.target_actor_identity?.monster_id),
      target_level: Number(sample.target_actor_identity?.level),
      target_attribute_state_id: Number(sample.target_attribute_state_id),
      target_attribute_ids: targetAttributeIds,
      target_physical_defense_attribute_id: PHYSICAL_DEFENSE_ATTRIBUTE_ID,
      target_physical_defense_observed: targetPhysicalDefense !== undefined,
      target_physical_defense_value: targetPhysicalDefense?.value ?? null,
      calculator_6500_curve_usable_for_this_hit: targetPhysicalDefense !== undefined,
      adjudication: targetPhysicalDefense === undefined
        ? "the exact packet omits Physical Defense 11350, so neither a guessed armor value nor the calculator's 6500 curve may be promoted for this hit"
        : "the packet carries a candidate armor input, but the curve and rounding still require controlled-pair proof",
    },
    selected_hit_backward_equation: {
      factual_output_damage: Number(sample.amount),
      packet_outcome: {
        critical: sample.critical === true,
        lucky: sample.lucky === true,
      },
      independently_filled_inputs: {
        physical_attack_11330: selectedWorkbench.source_physical_attack_11330,
        refined_physical_attack_11410: selectedWorkbench.source_refined_attack_11410,
        coefficient_basis_points: selectedWorkbench.damage_stage_coefficient_basis_points,
        fixed_damage_units: selectedWorkbench.damage_stage_fixed,
        critical_multiplier_raw_12510: selectedWorkbench.source_critical_multiplier_12510,
        light_bonus_raw_13170: selectedWorkbench.source_light_bonus_13170,
      },
      symbolic_equation:
        "117566 = FinalIntegerBoundary(((((DefenseAdjustedAttack(7070, target_physical_defense_11350) + RefinedAttackTerm(672) + ElementalOrDefenseFreeAttack) * 34500/10000) + 34) * VersatilityBucket * ElementalBucket * GenericBucket * SeasonalBucket * PhysicalBucket * OtherBucket * FinalBucket * CriticalOutcome(12510)))",
      unresolved_independent_terms: [
        "target_physical_defense_11350 and its exact transform",
        "elemental or other defense-free Attack term",
        "versatility bucket",
        "elemental bucket",
        "generic bucket",
        "seasonal bucket",
        "physical-damage bucket",
        "other and final buckets",
        "integer boundary between stages and at final output",
      ],
      identifiability:
        "one factual output can solve only the combined residual of these terms; it cannot uniquely assign values to multiple unknown factors without additional independent or controlled equations",
      residual_fit_may_be_promoted: false,
      closure_method:
        "add packet equations with the same unknown terms and exactly one changed proven input, then solve or cancel shared terms; reject any system whose unknowns remain rank-deficient",
    },
    current_build_trace_replication: {
      independent_capture_count: traceReplications.length,
      sessions: traceReplications,
      accepted_damage_rows: traceReplications.reduce(
        (sum, entry) => sum + entry.accepted_damage_rows,
        0,
      ),
      observed_damage: traceReplications.reduce(
        (sum, entry) => sum + BigInt(entry.observed_damage),
        0n,
      ).toString(),
      exact_contribution: {
        numerator: replicatedExactTotal.numerator.toString(),
        denominator: replicatedExactTotal.denominator.toString(),
        decimal: rationalDecimal(replicatedExactTotal),
      },
      conclusion:
        "factual final damage constrains the inverse equation independently in each capture; unlike packet states are not cross-paired, and the remaining downstream terms stay unresolved rather than being guessed",
      formula_authority: false,
    },
    existing_cross_coefficient_search: {
      ability_id: Number(crossCoefficientCohort.selection?.ability_ids?.[0] ?? 2_352),
      cohort_samples: crossCoefficientCohort.samples.length,
      observed_hit_event_ids: [...new Set(crossCoefficientCohort.samples.map((row) => Number(row.hit_event_id)))].sort(
        (left, right) => left - right,
      ),
      exact_state_key:
        "same build/session/run/scene/source/direct-source/target, source and target attribute/status state IDs, critical/lucky outcome, and packet formula fields before geometry",
      cross_hit_exact_state_groups: crossHitExactStateGroups,
      geometry_was_not_relaxed_to_create_a_match: true,
      conclusion: crossHitExactStateGroups === 0
        ? "the existing 3981-row ability cohort contains no exact shared-state group spanning two hit coefficients, so a latent downstream multiplier cannot be uniquely solved from this capture by cross-coefficient inversion"
        : "one or more cross-hit exact-state groups exist and require a separate geometry and integer-interval adjudication before use",
      formula_authority: false,
    },
    corrected_harmony_rounding_frontier: {
      accepted_damage_rows: trace.traces.length,
      observed_damage: trace.summary.observed_damage,
      exact_rational_total: {
        numerator: exactTotal.numerator.toString(),
        denominator: exactTotal.denominator.toString(),
        decimal_from_trace: trace.summary.exact_contribution.decimal,
      },
      aggregate_rounding_candidates: {
        floor_after_exact_sum: floorRational(exactTotal).toString(),
        ceil_after_exact_sum: ceilRational(exactTotal).toString(),
        nearest_half_up_after_exact_sum: nearestHalfUp(exactTotal).toString(),
      },
      per_damage_row_rounding_candidates: Object.fromEntries(
        Object.entries(perRowValues).map(([key, value]) => [key, value.toString()]),
      ),
      distinct_per_row_totals: distinctPerRowTotals.size,
      ambiguity_span: (perRowCeil - perRowFloor).toString(),
      formula_authority: false,
    },
    current_build_gate: {
      protocol_pack_id: pack.pack_id,
      protocol_routes: pack.routes.length,
      protocol_candidate_routes: pack.routes.filter((row) => row.confidence === "candidate").length,
      preflight_ready_for_snapshot: preflight.ready_for_snapshot,
      preflight_runtime_promotion_allowed: preflight.runtime_promotion_allowed,
      exact_final_server_integer_counterfactual_proven:
        acquisition.conclusion.exact_final_server_integer_counterfactual_proven,
    },
    next_exact_proof: {
      priority_ability_id: acquisition.identity.priority_ability_id,
      phases: acquisition.controlled_aba_contract.phases,
      minimum_repeats_per_phase: acquisition.controlled_aba_contract.minimum_repeats_per_phase,
      minimum_distinct_stage_signatures:
        acquisition.controlled_aba_contract.minimum_distinct_coefficient_stage_signatures,
      required_control:
        "same locally observed class-11 recipient and stationary target; repeat hit 1 and hit 3 before, during, and after one provider-owned effect-3003052 lifecycle while every non-Harmony source/target state remains unchanged",
      remote_player_cast_packet_required: false,
    },
    runtime_decision: {
      production_promotion_count: 0,
      promoted_effect_ids: [],
      provider_rdps_credit_allowed: false,
      reason: "the calculator closed the class-11 placement hypothesis but cannot supply the omitted target armor, prove downstream bucket purity, or select the final server integer boundary",
    },
    resource_bounds: {
      rss_samples: rssSamples,
      maximum_sampled_rss_bytes: Math.max(...rssSamples.map((row) => row.rss_bytes)),
      configured_ram_ceiling_bytes: 36 * 1024 ** 3,
    },
  };
  report.resource_bounds.sampled_rss_within_configured_ceiling =
    report.resource_bounds.maximum_sampled_rss_bytes <= report.resource_bounds.configured_ram_ceiling_bytes;
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify(summary(report), null, 2)}\n`);
}

function verify(values) {
  const inputPath = path.resolve(required(values, "input"));
  const report = readJson(inputPath);
  verifyReport(report);
  assert.equal(report.content_sha256, contentHash(report));
  process.stdout.write(`${JSON.stringify(summary(report), null, 2)}\n`);
}

function verifyReport(report) {
  assert.equal(SUPPORTED_SCHEMA_VERSIONS.has(Number(report.schema_version)), true);
  assert.equal(report.generated_by, "tools/bpsr-harmony-calculator-stage-adjudication.mjs");
  assert.equal(String(report.game_build), GAME_BUILD);
  assert.equal(Number(report.effect_id), EFFECT_ID);
  assert.equal(report.external_source.commit, EXTERNAL_COMMIT);
  assert.equal(report.policy.runtime_promotion_allowed, false);
  assert.equal(report.selected_hit_target_boundary.target_physical_defense_observed, false);
  assert.equal(report.selected_hit_target_boundary.calculator_6500_curve_usable_for_this_hit, false);
  assert.equal(report.existing_cross_coefficient_search.cross_hit_exact_state_groups, 0);
  assert.equal(report.current_build_gate.preflight_ready_for_snapshot, true);
  assert.equal(report.current_build_gate.preflight_runtime_promotion_allowed, false);
  assert.equal(report.current_build_gate.exact_final_server_integer_counterfactual_proven, false);
  assert.equal(report.next_exact_proof.remote_player_cast_packet_required, false);
  assert.equal(report.runtime_decision.production_promotion_count, 0);
  assert.deepEqual(report.runtime_decision.promoted_effect_ids, []);
  assert.equal(report.runtime_decision.provider_rdps_credit_allowed, false);
  assert.equal(report.resource_bounds.sampled_rss_within_configured_ceiling, true);
  if (Number(report.schema_version) >= 2) {
    assert.ok(report.current_build_trace_replication.independent_capture_count >= 1);
    assert.ok(report.current_build_trace_replication.accepted_damage_rows >= 1);
    assert.equal(report.current_build_trace_replication.formula_authority, false);
  }
  const stages = new Map(report.stage_adjudication.map((row) => [row.stage, row]));
  assert.equal(stages.get("class_11_primary_to_attack")?.closed_for_selected_scope, true);
  assert.equal(stages.get("target_mitigation_and_multiplier_buckets")?.closed_for_selected_scope, false);
  assert.equal(stages.get("final_server_integer")?.closed_for_selected_scope, false);
}

function summary(report) {
  return {
    game_build: report.game_build,
    effect_id: report.effect_id,
    accepted_damage_rows: report.corrected_harmony_rounding_frontier.accepted_damage_rows,
    exact_contribution_decimal:
      report.corrected_harmony_rounding_frontier.exact_rational_total.decimal_from_trace,
    aggregate_rounding_candidates:
      report.corrected_harmony_rounding_frontier.aggregate_rounding_candidates,
    per_damage_row_rounding_candidates:
      report.corrected_harmony_rounding_frontier.per_damage_row_rounding_candidates,
    ambiguity_span: report.corrected_harmony_rounding_frontier.ambiguity_span,
    replicated_capture_count:
      report.current_build_trace_replication?.independent_capture_count ?? 1,
    replicated_accepted_damage_rows:
      report.current_build_trace_replication?.accepted_damage_rows ??
      report.corrected_harmony_rounding_frontier.accepted_damage_rows,
    replicated_exact_contribution_decimal:
      report.current_build_trace_replication?.exact_contribution?.decimal ??
      report.corrected_harmony_rounding_frontier.exact_rational_total.decimal_from_trace,
    target_physical_defense_observed:
      report.selected_hit_target_boundary.target_physical_defense_observed,
    production_promotion_count: report.runtime_decision.production_promotion_count,
    content_sha256: report.content_sha256,
  };
}

function selfTest() {
  const left = { numerator: 1n, denominator: 3n };
  const right = { numerator: 1n, denominator: 6n };
  assert.deepEqual(addRational(left, right), { numerator: 1n, denominator: 2n });
  assert.equal(floorRational({ numerator: 15n, denominator: 10n }), 1n);
  assert.equal(ceilRational({ numerator: 15n, denominator: 10n }), 2n);
  assert.equal(nearestHalfUp({ numerator: 15n, denominator: 10n }), 2n);
  assert.equal(nearestHalfUp({ numerator: 14n, denominator: 10n }), 1n);
  process.stdout.write("self-test passed\n");
}

function summarizeTraceEquation(trace) {
  let exact = { numerator: 0n, denominator: 1n };
  for (const row of trace.traces) exact = addRational(exact, rationalFromTrace(row));
  assert.equal(exact.numerator.toString(), trace.summary.exact_contribution.numerator);
  assert.equal(exact.denominator.toString(), trace.summary.exact_contribution.denominator);
  return {
    session_id: trace.session_id,
    accepted_damage_rows: trace.traces.length,
    observed_damage: String(trace.summary.observed_damage),
    exact_contribution: {
      numerator: exact.numerator.toString(),
      denominator: exact.denominator.toString(),
      decimal: trace.summary.exact_contribution.decimal,
    },
  };
}

function rationalDecimal(value, digits = 6) {
  const scale = 10n ** BigInt(digits);
  const scaled = value.numerator * scale / value.denominator;
  const whole = scaled / scale;
  const fraction = (scaled % scale).toString().padStart(digits, "0");
  return `${whole}.${fraction}`;
}

function usage(exitCode) {
  process.stderr.write(
    "usage:\n" +
    "  node tools/bpsr-harmony-calculator-stage-adjudication.mjs self-test\n" +
    "  node tools/bpsr-harmony-calculator-stage-adjudication.mjs generate --trace <trace.json> [--replication-trace <trace.json>] --integer-acquisition <receipt.json> --formula-cohort <selected-hit-cohort.json> --cross-coefficient-cohort <ability-2352-cohort.json> [--output <report.json>]\n" +
    "  node tools/bpsr-harmony-calculator-stage-adjudication.mjs verify --input <report.json>\n",
  );
  process.exit(exitCode);
}
