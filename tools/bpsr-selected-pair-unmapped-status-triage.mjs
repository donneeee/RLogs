#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-selected-pair-unmapped-status-triage.mjs";
const EFFECT_ID = 55_228;
const ACTION_ID = 2_203_291;
const PRESENT_SEQUENCE = 55_702;
const ABSENT_SEQUENCE = 57_683;
const DAMAGE_OR_STAT_ZONES = new Set([
  "allRoundDamage",
  "baseAttackTerm",
  "critical",
  "elementalDamage",
  "generalDamage",
  "luckyEnhancement",
  "seasonSuppression",
  "skillMultiplier",
  "timingCadence",
]);

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
    modifier_classification_table: resolved(parsed, "classification"),
    modifier_formula_term_runtime: resolved(parsed, "formula-runtime"),
    modifier_value_proof_runtime: resolved(parsed, "value-proof"),
    buff_table: resolved(parsed, "buff-table"),
    attr_description: resolved(parsed, "attr-description"),
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
  assert(ledger?.schema_version === 6 &&
    ledger?.generated_by === "tools/bpsr-target-vulnerability-selected-pair-factor-ledger.mjs",
  "Selected-pair factor ledger identity mismatch");
  assert(String(ledger?.game_build) === buildId && ledger?.effect_id === EFFECT_ID &&
    ledger?.action_id === ACTION_ID, "Selected-pair factor ledger build/effect/action mismatch");
  assert(ledger?.selected_pair?.present_sequence === PRESENT_SEQUENCE &&
    ledger?.selected_pair?.absent_sequence === ABSENT_SEQUENCE,
  "Selected-pair sequence identity mismatch");

  const inventory = ledger.selected_pair_active_modifier_inventory;
  assert(inventory?.shared_status_instances === 120 &&
    inventory?.unmapped_status_instances?.length === 45,
  "Selected-pair unmapped inventory frontier mismatch");

  const classification = documents.modifier_classification_table;
  assert(classification?.schemaVersion === 1 && classification?.byBuffId &&
    classification?.sourcesByRuleId,
  "Modifier classification table identity mismatch");
  const formulaRuntime = documents.modifier_formula_term_runtime;
  assert(formulaRuntime?.schemaVersion === 1 && formulaRuntime?.entriesByKey,
    "Modifier formula-term runtime identity mismatch");
  const valueProof = documents.modifier_value_proof_runtime;
  assert(valueProof?.schemaVersion === 1 && valueProof?.entriesByKey,
    "Modifier value-proof runtime identity mismatch");

  const instancesByEffectId = new Map();
  for (const instance of inventory.unmapped_status_instances) {
    const rows = instancesByEffectId.get(instance.effect_id) ?? [];
    rows.push({
      locus: instance.locus,
      source_entity_uuid: instance.source_entity_uuid,
      stacks: instance.stacks,
      level: instance.level,
      origin_source_type_id: instance.origin_source_type_id,
      origin_source_config_id: instance.origin_source_config_id,
    });
    instancesByEffectId.set(instance.effect_id, rows);
  }

  const effects = [...instancesByEffectId.entries()].sort(([left], [right]) => left - right)
    .map(([effectId, instances]) => triageEffect(effectId, instances, documents));
  const sourceInstances = inventory.unmapped_status_instances.filter((row) =>
    row.locus === "source").length;
  const targetInstances = inventory.unmapped_status_instances.filter((row) =>
    row.locus === "target").length;
  const formulaEntries = effects.filter((row) => row.static_formula_term_runtime !== null);
  const valueEntries = effects.filter((row) => row.static_value_proof_runtime !== null);
  const classificationEntries = effects.filter((row) => row.classification_rule_ids.length > 0);
  const currentHpSemantic = effects.filter((row) => row.semantic_current_hp_terms.length > 0);
  const targetCurrentHpSemantic = currentHpSemantic.filter((row) =>
    row.instances.some((instance) => instance.locus === "target"));
  const damageOrStatCandidates = effects.filter((row) =>
    row.static_damage_or_stat_formula_zone_candidates.length > 0);
  const noStaticRoute = effects.filter((row) =>
    row.static_formula_term_runtime === null && row.classification_rule_ids.length === 0 &&
      row.static_value_proof_runtime === null);

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
      only_changed_target_attribute_id:
        ledger.exact_selected_target_attribute_context.only_changed_attribute_id,
      only_changed_target_attribute_is_current_hp:
        ledger.exact_selected_target_attribute_context.only_changed_attribute_is_current_hp,
    },
    inputs,
    policy: {
      exact_numeric_ids_and_build_identity_authoritative: true,
      localized_names_are_semantic_evidence_only: true,
      static_formula_routes_are_not_server_runtime_authority: true,
      absence_from_static_indexes_is_not_proof_of_no_effect: true,
      same_status_identity_is_not_proof_of_same_formula_contribution: true,
      unresolved_effects_preserved: true,
      provider_rdps_credit_fail_closed: true,
    },
    summary: {
      shared_status_instances_in_pair: inventory.shared_status_instances,
      formerly_unmapped_status_instances: inventory.unmapped_status_instances.length,
      formerly_unmapped_distinct_effect_ids: effects.length,
      source_instances: sourceInstances,
      target_instances: targetInstances,
      current_build_classification_entries: classificationEntries.length,
      current_build_formula_term_entries: formulaEntries.length,
      current_build_value_proof_entries: valueEntries.length,
      static_damage_or_stat_formula_zone_candidates: damageOrStatCandidates.length,
      no_static_route_entries: noStaticRoute.length,
      semantic_current_hp_candidates: currentHpSemantic.length,
      target_locus_semantic_current_hp_candidates: targetCurrentHpSemantic.length,
      all_effect_ids_retained: effects.length === instancesByEffectId.size,
      exact_server_current_hp_independence_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
    effects,
    remaining_proof_obligations: [
      "adjudicate each static formula-zone candidate against the exact selected action, target, source state, and current-build runtime operator",
      "prove that target current HP does not affect action 2203291 / damage attribute 2220329109 directly or through any active target-side status",
      "deduplicate parent, child, counter, and provider-scoped manifestations before composing the selected-pair baseline",
      "prove exact operation order, stacking, and every integer rounding stage before assigning provider credit",
    ],
    promotion_counts: {
      production_runtime_ui: 0,
      superseded_historical: 2,
      active_current_build_candidates: 1,
    },
  };
}

function triageEffect(effectId, instances, documents) {
  const buff = documents.buff_table?.[String(effectId)] ?? null;
  assert(buff === null || Number(buff.Id) === effectId, `BuffTable identity mismatch for ${effectId}`);
  const tipsId = Number(buff?.TipsDescription ?? 0);
  const tips = tipsId > 0 ? documents.attr_description?.[String(tipsId)] ?? null : null;
  const classificationRuleIds = array(documents.modifier_classification_table
    ?.byBuffId?.[String(effectId)]).filter((value) => typeof value === "string");
  const classificationRules = classificationRuleIds.map((ruleId) => {
    const rule = documents.modifier_classification_table.sourcesByRuleId?.[ruleId];
    assert(rule, `Missing classification rule ${ruleId} for effect ${effectId}`);
    return {
      source_rule_id: ruleId,
      source_id: rule.sourceId ?? null,
      source_type: rule.sourceType ?? null,
      report_policy: rule.reportPolicy ?? null,
      row_model: rule.rowModel ?? null,
      primary_role: rule.primaryRole ?? null,
      report_domains: array(rule.reportDomains),
      contribution_status: rule.contributionStatus ?? null,
      classification_tags: array(rule.classificationTags),
    };
  });
  const formula = documents.modifier_formula_term_runtime?.entriesByKey?.[`buffs:${effectId}`] ?? null;
  const proof = documents.modifier_value_proof_runtime?.entriesByKey?.[`buffs:${effectId}`] ?? null;
  const formulaZones = array(formula?.formulaZoneIds);
  const semanticFields = [buff?.NameDesign, buff?.Name, buff?.Desc, tips?.Description]
    .filter((value) => typeof value === "string" && value.length > 0);
  const semanticCurrentHpTerms = currentHpTerms(semanticFields.join("\n"));
  const damageOrStatZones = formulaZones.filter((zone) => DAMAGE_OR_STAT_ZONES.has(zone));
  const disposition = damageOrStatZones.length > 0
    ? "static_damage_or_stat_route_candidate"
    : formula !== null
      ? formula.formulaReadiness === "non-damage-or-support"
        ? "static_non_damage_or_support_candidate"
        : "static_formula_entry_without_damage_or_stat_zone"
      : classificationRuleIds.length > 0
        ? "classification_only_no_formula_entry"
        : "no_current_build_static_route_entry";
  return {
    effect_id: effectId,
    instances,
    label_evidence: buff === null ? null : {
      name_design: buff.NameDesign ?? "",
      localized_name: buff.Name ?? "",
      localized_description: buff.Desc ?? "",
      tips_description_id: tipsId || null,
      tips_description: tips?.Description ?? null,
      buff_ability_type: buff.BuffAbilityType ?? null,
      buff_ability_sub_type: buff.BuffAbilitySubType ?? null,
      tags: array(buff.Tags),
      destroy_param: buff.DestroyParam ?? null,
    },
    classification_rule_ids: classificationRuleIds,
    classification_rules: classificationRules,
    static_formula_term_runtime: formula === null ? null : {
      key: `buffs:${effectId}`,
      formula_readiness: formula.formulaReadiness ?? null,
      formula_zone_ids: formulaZones,
      value_resolution: formula.valueResolution ?? null,
      scope_kinds: array(formula.scopeKinds),
      stack_policy: formula.stackPolicy ?? null,
      runtime_proof_required: array(formula.runtimeProofRequired),
    },
    static_value_proof_runtime: proof === null ? null : {
      key: `buffs:${effectId}`,
      value_proof_status: proof.valueProofStatus ?? null,
      selected_values: array(proof.selectedValues),
      value_blockers: array(proof.valueBlockers),
      proof_requirements: array(proof.proofRequirements),
    },
    static_damage_or_stat_formula_zone_candidates: damageOrStatZones,
    semantic_current_hp_terms: semanticCurrentHpTerms,
    triage_disposition: disposition,
    exact_selected_action_applicability_proven: false,
    exact_runtime_formula_stage_proven: false,
    exact_current_hp_dependency_or_independence_proven: false,
    provider_rdps_credit_allowed: false,
  };
}

function currentHpTerms(text) {
  const candidates = [
    ["current-hp", /\bcurrent\s+hp\b/i],
    ["hp", /\bhp\b/i],
    ["life", /\blife\b/i],
    ["chinese-life", /生命/u],
    ["chinese-hp", /血量/u],
  ];
  return candidates.filter(([, expression]) => expression.test(text)).map(([key]) => key);
}

function verifyCommand(parsed) {
  const input = resolved(parsed, "input");
  const report = readJson(input, "input");
  verifyReport(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  assert(report?.schema_version === SCHEMA_VERSION && report?.generated_by === GENERATOR,
    "Triage report identity mismatch");
  assert(report?.effect_id === EFFECT_ID && report?.action_id === ACTION_ID &&
    report?.selected_pair?.present_sequence === PRESENT_SEQUENCE &&
    report?.selected_pair?.absent_sequence === ABSENT_SEQUENCE,
  "Triage selected-pair identity mismatch");
  assert(report?.summary?.formerly_unmapped_status_instances === 45 &&
    report?.summary?.formerly_unmapped_distinct_effect_ids === 43 &&
    report?.summary?.source_instances === 36 && report?.summary?.target_instances === 9 &&
    report?.effects?.length === 43,
  "Triage status census mismatch");
  assert(new Set(report.effects.map((row) => row.effect_id)).size === 43 &&
    report.effects.every((row) => row.provider_rdps_credit_allowed === false),
  "Triage did not retain every unique fail-closed effect");
  assert(report?.summary?.current_build_classification_entries === 24 &&
    report?.summary?.current_build_formula_term_entries === 32 &&
    report?.summary?.current_build_value_proof_entries === 10 &&
    report?.summary?.no_static_route_entries === 11,
  "Triage current-build static coverage mismatch");
  assert(report?.policy?.localized_names_are_semantic_evidence_only === true &&
    report?.policy?.static_formula_routes_are_not_server_runtime_authority === true &&
    report?.summary?.exact_server_current_hp_independence_proven === false &&
    report?.summary?.provider_rdps_credit_allowed === false &&
    report?.summary?.runtime_promotion_allowed === false &&
    report?.promotion_counts?.production_runtime_ui === 0,
  "Triage granted unsafe promotion authority");
  if (report.content_sha256 !== undefined) {
    assert(report.content_sha256 === contentHash(report), "Triage content hash mismatch");
  }
}

function selfTest() {
  assert(currentHpTerms("When HP changes").includes("hp"), "HP token self-test failed");
  assert(currentHpTerms("Current HP +3%").includes("current-hp"),
    "Current HP token self-test failed");
  assert(currentHpTerms("movement speed").length === 0,
    "Non-HP token self-test failed");
  assert(array(null).length === 0 && array(3)[0] === 3 && array([1, 2]).length === 2,
    "Array normalization self-test failed");
  console.log("bpsr-selected-pair-unmapped-status-triage self-test passed");
}

function parseArgs(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    if (!flag?.startsWith("--") || value === undefined) usage(1);
    parsed[flag.slice(2)] = value;
  }
  return parsed;
}

function required(parsed, key) {
  const value = parsed[key];
  if (value === undefined || value === "") throw new Error(`Missing --${key}`);
  return value;
}

function resolved(parsed, key) {
  const value = path.resolve(required(parsed, key));
  if (!existsSync(value)) throw new Error(`Missing ${key}: ${value}`);
  return value;
}

function numericString(value, label) {
  assert(/^\d+$/.test(String(value)), `${label} must be numeric`);
  return String(value);
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`Unable to read ${label} ${file}: ${error.message}`);
  }
}

function descriptor(file) {
  return {
    path: file.replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: `sha256:${createHash("sha256").update(readFileSync(file)).digest("hex")}`,
  };
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return `sha256:${createHash("sha256").update(stableStringify(copy)).digest("hex")}`;
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function array(value) {
  if (value === undefined || value === null) return [];
  return Array.isArray(value) ? value : [value];
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(code) {
  console.log("Usage:\n  node tools/bpsr-selected-pair-unmapped-status-triage.mjs build --build <id> --ledger <json> --classification <json> --formula-runtime <json> --value-proof <json> --buff-table <json> --attr-description <json> --output <json>\n  node tools/bpsr-selected-pair-unmapped-status-triage.mjs verify --input <json>\n  node tools/bpsr-selected-pair-unmapped-status-triage.mjs self-test");
  process.exit(code);
}
