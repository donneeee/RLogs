#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-selected-pair-current-hp-route-proof.mjs";
const EFFECT_ID = 55_228;
const ACTION_ID = 2_203_291;
const PRESENT_SEQUENCE = 55_702;
const ABSENT_SEQUENCE = 57_683;
const CURRENT_HP_ATTRIBUTE_ID = 11_310;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(parsed) {
  const buildId = numericString(required(parsed, "build"), "build");
  const files = {
    selected_pair_factor_ledger: resolved(parsed, "ledger"),
    selected_pair_formula_context: resolved(parsed, "formula-context"),
    unmapped_status_triage: resolved(parsed, "unmapped-status-triage"),
    state_dependent_mechanic_inventory: resolved(parsed, "state-inventory"),
    modifier_source_index: resolved(parsed, "modifier-source-index"),
    modifier_formula_term_runtime: resolved(parsed, "formula-runtime"),
    modifier_value_proof_runtime: resolved(parsed, "value-proof"),
  };
  const output = path.resolve(required(parsed, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);
  const documents = Object.fromEntries(Object.entries(files).map(([key, file]) =>
    [key, readJson(file, key)]));
  const inputs = Object.fromEntries(Object.entries(files).map(([key, file]) =>
    [key, descriptor(file)]));
  const report = buildReport(buildId, documents, inputs);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote ${output}`);
}

function buildReport(buildId, documents, inputs) {
  const ledger = documents.selected_pair_factor_ledger;
  assert(ledger?.schema_version === 7 &&
    ledger?.generated_by === "tools/bpsr-target-vulnerability-selected-pair-factor-ledger.mjs" &&
    String(ledger?.game_build) === buildId && ledger?.effect_id === EFFECT_ID &&
    ledger?.action_id === ACTION_ID, "Selected-pair factor ledger identity mismatch");
  assert(ledger?.selected_pair?.present_sequence === PRESENT_SEQUENCE &&
    ledger?.selected_pair?.absent_sequence === ABSENT_SEQUENCE,
  "Selected-pair factor ledger sequence identity mismatch");

  const context = documents.selected_pair_formula_context;
  assert(context?.schema_version === 2 &&
    context?.generated_by === "tools/bpsr-selected-pair-formula-context.mjs" &&
    String(context?.game_build) === buildId && context?.selection?.action_id === ACTION_ID &&
    context?.selection?.present_sequence === PRESENT_SEQUENCE &&
    context?.selection?.absent_sequence === ABSENT_SEQUENCE,
  "Selected-pair formula context identity mismatch");
  const samples = array(context.selected_samples);
  assert(samples.length === 2 && samples.every((row) => row.source_attribute_state_id === 40_685),
    "Selected-pair source attribute state identity mismatch");
  const targetChanges = array(context.target_attribute_state_comparison?.changed_attributes);
  assert(targetChanges.length === 1 && targetChanges[0]?.attribute_id === CURRENT_HP_ATTRIBUTE_ID,
    "Selected-pair target attribute difference is no longer current HP only");

  const triage = documents.unmapped_status_triage;
  assert(triage?.schema_version === 1 &&
    triage?.generated_by === "tools/bpsr-selected-pair-unmapped-status-triage.mjs" &&
    String(triage?.game_build) === buildId && triage?.effect_id === EFFECT_ID &&
    triage?.action_id === ACTION_ID, "Unmapped-status triage identity mismatch");
  const semanticHpEffects = array(triage.effects)
    .filter((row) => array(row.semantic_current_hp_terms).length > 0)
    .map((row) => semanticEffectRoute(row, context));
  assert(semanticHpEffects.length === 3 &&
    semanticHpEffects.every((row) => row.loci.length === 1 && row.loci[0] === "source"),
  "Selected-pair semantic current-HP frontier changed");

  const inventory = documents.state_dependent_mechanic_inventory;
  assert(inventory?.schema_version === 5 &&
    inventory?.generated_by === "rlogs-bpsr-state-dependent-mechanic-inventory" &&
    String(inventory?.client_build) === buildId,
  "State-dependent mechanic inventory identity mismatch");
  const activeStatuses = array(ledger.selected_pair_active_modifier_inventory?.status_mappings);
  const activeEffectIds = new Set(activeStatuses.map((row) => Number(row.effect_id)));
  const healthDirectSources = array(inventory.direct_sources)
    .filter((row) => array(row.signals).some((signal) =>
      ["current_health_scaling_or_gate", "maximum_health_scaling_or_gate",
        "missing_health_scaling_or_gate", "health_threshold_or_gate"].includes(signal)))
    .filter((row) => array(row.related_buff_ids).some((id) => activeEffectIds.has(Number(id))));
  assert(healthDirectSources.length === 1 && healthDirectSources[0]?.source_id ===
    "phantom-factor:3059050", "Active state-dependent direct-source intersection changed");

  const sourceIndex = documents.modifier_source_index;
  assert(sourceIndex?.schemaVersion === 1 && sourceIndex?.byBuffId,
    "Modifier source index identity mismatch");
  const formulaRuntime = documents.modifier_formula_term_runtime;
  assert(formulaRuntime?.schemaVersion === 1 && formulaRuntime?.entriesByKey,
    "Modifier formula-term runtime identity mismatch");
  const valueProof = documents.modifier_value_proof_runtime;
  assert(valueProof?.schemaVersion === 1 && valueProof?.entriesByKey,
    "Modifier value-proof runtime identity mismatch");

  const stateDependentRoutes = healthDirectSources.map((source) => stateDependentRoute({
    source,
    activeStatuses,
    sourceIndex,
    formulaRuntime,
    valueProof,
  }));
  assert(stateDependentRoutes.every((row) => row.selected_outgoing_damage_disposition ===
    "excluded-current-build-defensive-owner-route"),
  "A selected state-dependent route is no longer strictly defensive");

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: buildId,
    effect_id: EFFECT_ID,
    action_id: ACTION_ID,
    selected_pair: {
      present_sequence: PRESENT_SEQUENCE,
      absent_sequence: ABSENT_SEQUENCE,
      present_amount: ledger.selected_pair.present_amount,
      absent_amount: ledger.selected_pair.absent_amount,
      observed_delta: ledger.selected_pair.observed_delta,
      source_entity_uuid: samples[0].source_entity_uuid,
      target_entity_uuid: samples[0].target_entity_uuid,
      source_attribute_state_id: samples[0].source_attribute_state_id,
      source_attribute_vectors_equal: samples[0].source_attribute_state_id ===
        samples[1].source_attribute_state_id,
      only_changed_target_attribute_id: CURRENT_HP_ATTRIBUTE_ID,
      target_current_hp: {
        present: targetChanges[0].present_value,
        absent: targetChanges[0].absent_value,
        delta_present_minus_absent: targetChanges[0].delta_present_minus_absent,
      },
    },
    inputs,
    policy: {
      exact_numeric_ids_and_build_identity_authoritative: true,
      localized_descriptions_are_semantic_direction_evidence_not_server_formula_authority: true,
      broad_health_and_damage_text_cooccurrence_is_not_an_outgoing_damage_route: true,
      shared_source_status_and_identical_source_attribute_state_is_common_baseline_evidence: true,
      absence_of_target_locus_catalog_candidates_is_not_global_server_independence_proof: true,
      unresolved_intrinsic_action_target_hp_behavior_is_preserved: true,
      provider_rdps_credit_fail_closed: true,
    },
    semantic_current_hp_status_routes: semanticHpEffects,
    state_dependent_direct_source_routes: stateDependentRoutes,
    summary: {
      semantic_current_hp_status_candidates: semanticHpEffects.length,
      semantic_current_hp_source_locus_candidates: semanticHpEffects.filter((row) =>
        row.loci.includes("source")).length,
      semantic_current_hp_target_locus_candidates: semanticHpEffects.filter((row) =>
        row.loci.includes("target")).length,
      active_state_dependent_direct_source_intersections: stateDependentRoutes.length,
      defensive_owner_routes_excluded_from_selected_outgoing_damage: stateDependentRoutes.filter(
        (row) => row.selected_outgoing_damage_disposition ===
          "excluded-current-build-defensive-owner-route").length,
      selected_outgoing_health_dependent_catalog_routes_remaining: 0,
      exact_selected_action_target_current_hp_independence_proven: false,
      exact_selected_action_target_current_hp_dependency_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
    narrowed_open_proof_obligations: [
      "test action 2203291 / damage attribute 2220329109 across controlled exact-build observations spanning target current-HP states and candidate effect 55228 states",
      "prove whether the server action formula has an intrinsic target-current-HP term or threshold not represented by active status catalogs",
      "only after current-HP independence or an exact current-HP operator is proven, solve effect 55228 magnitude, operation order, stacking, and integer rounding",
      "replay the counterfactual with exact integer conservation before assigning provider rDPS credit",
    ],
    promotion_counts: {
      production_runtime_ui: 0,
      superseded_historical: 2,
      active_current_build_candidates: 1,
    },
  };
}

function semanticEffectRoute(effect, context) {
  const loci = unique(array(effect.instances).map((row) => String(row.locus)));
  const sameSourceAttributeState = array(context.selected_samples).every((row) =>
    row.source_attribute_state_id === context.selected_samples[0].source_attribute_state_id);
  const formulaZones = array(effect.static_damage_or_stat_formula_zone_candidates);
  let disposition = "source-status-common-baseline-exact-delta-effect-unproven";
  if (effect.static_formula_term_runtime?.formula_readiness === "non-damage-or-support" &&
      formulaZones.length === 0) {
    disposition = "source-status-non-damage-or-support-route";
  } else if (sameSourceAttributeState && loci.length === 1 && loci[0] === "source") {
    disposition = "source-stat-route-common-baseline-identical-attribute-snapshot";
  }
  return {
    effect_id: effect.effect_id,
    loci,
    instances: structuredClone(effect.instances),
    label_evidence: structuredClone(effect.label_evidence),
    semantic_current_hp_terms: structuredClone(effect.semantic_current_hp_terms),
    formula_readiness: effect.static_formula_term_runtime?.formula_readiness ?? null,
    formula_zone_ids: structuredClone(effect.static_formula_term_runtime?.formula_zone_ids ?? []),
    value_proof_status: effect.static_value_proof_runtime?.value_proof_status ?? null,
    source_attribute_vectors_equal_for_selected_pair: sameSourceAttributeState,
    disposition,
    exact_server_formula_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function stateDependentRoute({ source, activeStatuses, sourceIndex, formulaRuntime, valueProof }) {
  const relatedBuffIds = unique(array(source.related_buff_ids).map(Number));
  const activeInstances = activeStatuses.filter((row) => relatedBuffIds.includes(Number(row.effect_id)));
  const sourceRecords = relatedBuffIds.flatMap((id) => array(sourceIndex.byBuffId?.[String(id)]));
  assert(sourceRecords.length > 0, `${source.source_id} has no ModifierSourceIndex record`);
  const components = sourceRecords.flatMap((row) => array(row.attributionModel?.components));
  const uidEdges = sourceRecords.flatMap((row) => array(row.uidEdges));
  const strictOutgoingEdges = uidEdges.filter((edge) =>
    edge.role === "target" && ["damage", "recount"].includes(edge.uidKind));
  const defensiveOwnerOnly = sourceRecords.every((row) =>
    row.reportPolicy === "ignore" && row.rowPolicy === "defensive" &&
    row.contributionStatus === "defensive-or-non-damage") && components.length > 0 &&
    components.every((component) => component.direction === "damage-taken" &&
      component.contributionScope === "owner" &&
      component.transferEligibility === "self-only-formula-context") &&
    strictOutgoingEdges.length === 0;
  const runtimeRows = relatedBuffIds.map((id) => ({
    buff_id: id,
    formula_term_runtime: compactFormula(formulaRuntime.entriesByKey?.[`buffs:${id}`]),
    value_proof_runtime: compactValue(valueProof.entriesByKey?.[`buffs:${id}`]),
  }));
  assert(runtimeRows.every((row) =>
    row.formula_term_runtime?.formula_readiness === "non-damage-or-support" &&
    row.value_proof_runtime?.value_proof_status === "non-damage-or-support"),
  `${source.source_id} runtime indexes no longer classify the route as non-damage/support`);
  return {
    source_id: source.source_id,
    source_kind: source.source_kind,
    source_entity_id: source.source_entity_id,
    source_name_evidence: source.source_name,
    signals: structuredClone(source.signals),
    matching_description_text: structuredClone(source.matching_description_text),
    packet_evidence: structuredClone(source.packet_evidence),
    related_buff_ids: relatedBuffIds,
    active_instances: structuredClone(activeInstances),
    modifier_source_records: sourceRecords.map((row) => ({
      source_id: row.sourceId,
      source_type: row.sourceType,
      family_id: row.familyId ?? null,
      report_policy: row.reportPolicy,
      row_policy: row.rowPolicy,
      contribution_status: row.contributionStatus,
      predicate_tags: structuredClone(row.predicateTags ?? []),
      descriptions: structuredClone(row.descriptions ?? {}),
      attribution_components: structuredClone(row.attributionModel?.components ?? []),
      uid_edges: structuredClone(row.uidEdges ?? []),
    })),
    runtime_rows: runtimeRows,
    strict_target_damage_or_recount_edges: structuredClone(strictOutgoingEdges),
    selected_outgoing_damage_disposition: defensiveOwnerOnly
      ? "excluded-current-build-defensive-owner-route"
      : "unresolved-health-dependent-route",
    exact_server_formula_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function compactFormula(row) {
  if (!row) return null;
  return {
    formula_readiness: row.formulaReadiness ?? null,
    formula_zone_ids: structuredClone(row.formulaZoneIds ?? []),
    value_resolution: row.valueResolution ?? null,
    runtime_proof_required: structuredClone(row.runtimeProofRequired ?? []),
  };
}

function compactValue(row) {
  if (!row) return null;
  return {
    value_proof_status: row.valueProofStatus ?? null,
    formula_readiness: row.formulaReadiness ?? null,
    proof_requirements: structuredClone(row.proofRequirements ?? []),
  };
}

function verifyCommand(parsed) {
  const file = path.resolve(required(parsed, "input"));
  const report = readJson(file, "current-HP route proof");
  verifyReport(report);
  console.log(`verified ${file}`);
}

function verifyReport(report) {
  assert(report?.schema_version === SCHEMA_VERSION && report?.generated_by === GENERATOR,
    "Current-HP route proof identity mismatch");
  assert(report?.effect_id === EFFECT_ID && report?.action_id === ACTION_ID,
    "Current-HP route proof effect/action mismatch");
  assert(report?.selected_pair?.present_sequence === PRESENT_SEQUENCE &&
    report?.selected_pair?.absent_sequence === ABSENT_SEQUENCE,
  "Current-HP route proof sequence mismatch");
  assert(report?.summary?.semantic_current_hp_target_locus_candidates === 0 &&
    report?.summary?.active_state_dependent_direct_source_intersections === 1 &&
    report?.summary?.defensive_owner_routes_excluded_from_selected_outgoing_damage === 1 &&
    report?.summary?.selected_outgoing_health_dependent_catalog_routes_remaining === 0,
  "Current-HP route proof frontier mismatch");
  assert(report?.summary?.exact_selected_action_target_current_hp_independence_proven === false &&
    report?.summary?.provider_rdps_credit_allowed === false &&
    report?.summary?.runtime_promotion_allowed === false,
  "Current-HP route proof must fail closed");
  assert(report?.promotion_counts?.production_runtime_ui === 0 &&
    report?.promotion_counts?.superseded_historical === 2 &&
    report?.promotion_counts?.active_current_build_candidates === 1,
  "Promotion counts changed");
  assert(report?.content_sha256 === contentHash(report), "Content hash mismatch");
}

function selfTest() {
  const source = {
    source_id: "phantom-factor:3059050",
    source_kind: "phantom-factor",
    source_entity_id: 3059050,
    source_name: "label-only",
    signals: ["current_health_scaling_or_gate"],
    matching_description_text: [],
    packet_evidence: {},
    related_buff_ids: [3059050],
  };
  const row = stateDependentRoute({
    source,
    activeStatuses: [{ locus: "source", effect_id: 3059050 }],
    sourceIndex: { byBuffId: { "3059050": [{
      sourceId: "phantom-factor:3059050",
      sourceType: "season-phantom-factor",
      reportPolicy: "ignore",
      rowPolicy: "defensive",
      contributionStatus: "defensive-or-non-damage",
      attributionModel: { components: [{
        direction: "damage-taken",
        contributionScope: "owner",
        transferEligibility: "self-only-formula-context",
      }] },
      uidEdges: [{ edgeKind: "runtime-buff", uidKind: "buff", uid: 3059050,
        role: "runtime" }],
    }] } },
    formulaRuntime: { entriesByKey: { "buffs:3059050": {
      formulaReadiness: "non-damage-or-support",
    } } },
    valueProof: { entriesByKey: { "buffs:3059050": {
      formulaReadiness: "non-damage-or-support",
      valueProofStatus: "non-damage-or-support",
    } } },
  });
  assert(row.selected_outgoing_damage_disposition ===
    "excluded-current-build-defensive-owner-route", "Defensive route self-test failed");
  console.log("self-test passed");
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return `sha256:${createHash("sha256").update(`${JSON.stringify(copy)}\n`).digest("hex")}`;
}

function descriptor(file) {
  const bytes = readFileSync(file);
  return {
    path: file.replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: `sha256:${createHash("sha256").update(bytes).digest("hex")}`,
  };
}

function readJson(file, label) {
  if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`);
  return JSON.parse(readFileSync(file, "utf8"));
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) throw new Error(`Invalid argument near ${key}`);
    parsed[key.slice(2)] = value;
  }
  return parsed;
}

function required(parsed, key) {
  const value = parsed[key];
  if (!value) throw new Error(`Missing --${key}`);
  return value;
}

function resolved(parsed, key) {
  return path.resolve(required(parsed, key));
}

function numericString(value, label) {
  if (!/^\d+$/.test(value)) throw new Error(`${label} must contain only ASCII digits`);
  return value;
}

function unique(values) {
  return [...new Set(values)].sort((left, right) => typeof left === "number"
    ? left - right
    : String(left).localeCompare(String(right)));
}

function array(value) {
  return Array.isArray(value) ? value : [];
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(exitCode) {
  console.log("usage: bpsr-selected-pair-current-hp-route-proof.mjs <build|verify|self-test> [options]");
  process.exit(exitCode);
}
