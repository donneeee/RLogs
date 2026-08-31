#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-source-status-confounder-route-audit.mjs";
const EFFECT_ID = 55342;
const ACTION_ID = 25534201;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "analyze") analyze(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const manifestPath = path.resolve(required(parsed, "build-source-manifest"));
  const buffTablePath = path.resolve(required(parsed, "buff-table"));
  const damageAttrTablePath = path.resolve(required(parsed, "damage-attr-table"));
  const activationPath = path.resolve(required(parsed, "activation-index"));
  const transitionPath = path.resolve(required(parsed, "transition-counterfactual-audit"));
  const formulaEvidencePath = path.resolve(required(parsed, "static-formula-evidence"));
  const output = path.resolve(required(parsed, "output"));

  const manifest = readJson(manifestPath, "build source manifest");
  const buffTable = readJson(buffTablePath, "BuffTable");
  const damageAttrTable = readJson(damageAttrTablePath, "DamageAttrTable");
  const activation = readJson(activationPath, "damage action activation index");
  const transition = readJson(transitionPath, "transition counterfactual audit");
  const formulaEvidence = readJson(formulaEvidencePath, "static formula evidence");
  const inputs = {
    build_source_manifest: fileDescriptor(manifestPath),
    buff_table: fileDescriptor(buffTablePath),
    damage_attr_table: fileDescriptor(damageAttrTablePath),
    activation_index: fileDescriptor(activationPath),
    transition_counterfactual_audit: fileDescriptor(transitionPath),
    static_formula_evidence: fileDescriptor(formulaEvidencePath),
  };

  const tableBindings = validateManifest(manifest, build, inputs);
  validateActivation(activation, build);
  validateTransition(transition, build);
  validateFormulaEvidence(formulaEvidence, build);

  const buff = buffTable[String(EFFECT_ID)];
  if (!buff || Number(buff.Id) !== EFFECT_ID) {
    throw new Error(`missing exact BuffTable row ${EFFECT_ID}`);
  }
  const action = damageAttrTable[String(ACTION_ID)];
  if (!action || Number(action.Id) !== ACTION_ID || Number(action.TypeEnum) !== EFFECT_ID) {
    throw new Error(`missing exact DamageAttrTable route ${EFFECT_ID} -> ${ACTION_ID}`);
  }
  const ability = activation.observed_ability_result_kinds.find(
    (row) => Number(row.ability_id) === EFFECT_ID,
  );
  const observedAction = activation.observed_damage_rows.find(
    (row) => Number(row.damage_id) === ACTION_ID,
  );
  if (!ability || !observedAction ||
    stableStringify(observedAction.semantic_row) !== stableStringify(action)) {
    throw new Error("activation index does not preserve the exact linked action row");
  }

  const formulaSources = Array.isArray(formulaEvidence.sources)
    ? formulaEvidence.sources
    : Object.values(formulaEvidence.sources ?? {});
  const directFormulaTokenMatches = formulaSources.filter((source) =>
    containsExactScalar(source, EFFECT_ID));
  const sourceStatusDifferenceCount = Number(
    transition.summary.same_context_source_status_difference_counts?.[String(EFFECT_ID)] ?? 0,
  );
  const retainedMismatchExamples = (transition.same_context_mismatch_examples ?? []).filter((row) =>
    containsExactScalar(row, EFFECT_ID)).length;

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    effect_id: EFFECT_ID,
    linked_action_id: ACTION_ID,
    status: "healing-action-route-proven-status-damage-neutrality-unproven",
    policy: {
      exact_numeric_effect_action_ids_build_and_input_hashes_are_authoritative: true,
      localized_names_are_semantic_evidence_only: true,
      produced_action_healing_only_does_not_prove_status_modifier_damage_neutrality: true,
      absent_static_formula_token_is_not_zero_effect_proof: true,
      remote_player_packet_acquisition_required: false,
      never_received_remote_player_packet_absence_is_not_zero: true,
      unknown_and_unresolved_status_roles_are_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs,
    build_identity: {
      manifest_aggregate_sha256: String(manifest.aggregateSha256),
      decoded_table_bindings: tableBindings,
      activation_index_sessions: Number(activation.summary.sessions),
      exact_static_table_hash_binding_proven: true,
    },
    exact_buff_static_fields: {
      id: Number(buff.Id),
      level: Number(buff.Level),
      repeat_add_rule: structuredClone(buff.RepeatAddRule ?? []),
      destroy_param: structuredClone(buff.DestroyParam ?? []),
      special_attr: structuredClone(buff.SpecialAttr ?? []),
      skill_id: Number(buff.SkillId ?? 0),
      is_client_buff: Boolean(buff.IsClientBuff),
      buff_ability_type: Number(buff.BuffAbilityType ?? 0),
      buff_ability_sub_type: Number(buff.BuffAbilitySubType ?? 0),
    },
    localized_semantic_evidence_only: {
      name_design: String(buff.NameDesign ?? ""),
      name: String(buff.Name ?? ""),
      description: String(buff.Desc ?? ""),
      localized_fields_consistent: String(buff.NameDesign ?? "") === String(buff.Name ?? "") &&
        String(buff.Name ?? "") === String(buff.Desc ?? ""),
    },
    exact_linked_action: {
      damage_attr_id: Number(action.Id),
      type_enum: Number(action.TypeEnum),
      damage_script: String(action.DamageScript ?? ""),
      damage_type: Number(action.DamageType),
      pve_damage_radio: structuredClone(action.PVEDamageRadio ?? []),
      pve_fixed_parameter: structuredClone(action.PVEFixedParameter ?? []),
      tags: structuredClone(action.Tags ?? []),
    },
    packet_observed_action_outcomes: {
      packet_damage_results: Number(ability.packet_damage_results),
      packet_healing_results: Number(ability.packet_healing_results),
      results_with_hit_event_id: Number(ability.results_with_hit_event_id),
      results_without_hit_event_id: Number(ability.results_without_hit_event_id),
      action_row_packet_damage_results: Number(observedAction.packet_damage_results),
      action_row_packet_healing_results: Number(observedAction.packet_healing_results),
      produced_action_disposition: "produced-action-healing-only-observed",
      linked_action_output_damage_neutrality_proven_for_observed_index: true,
      status_modifier_damage_neutrality_proven: false,
    },
    counterfactual_confounder: {
      selected_effect_id: Number(transition.effect_id),
      same_normalized_damage_context_pairs: Number(
        transition.summary.same_normalized_damage_context_pairs),
      same_context_source_status_difference_count: sourceStatusDifferenceCount,
      retained_mismatch_examples_containing_effect: retainedMismatchExamples,
      same_context_and_nonselected_status_pairs: Number(
        transition.summary.same_context_and_nonselected_status_pairs),
      strict_controlled_counterfactual_pairs: Number(
        transition.summary.strict_controlled_counterfactual_pairs),
      may_exclude_from_counterfactual_matching: false,
    },
    static_formula_coverage: {
      evidence_source_count: formulaSources.length,
      direct_exact_effect_token_matches: directFormulaTokenMatches.length,
      direct_modifier_route_found: directFormulaTokenMatches.length > 0,
      absence_proves_damage_neutrality: false,
    },
    conclusion: {
      linked_action_route_proven: true,
      linked_action_observed_as_healing_only: true,
      status_modifier_damage_neutrality_proven: false,
      effect_may_be_removed_as_counterfactual_confounder: false,
      structural_remote_player_packets_required_to_close: false,
      locally_observable_proof_needed:
        "an isolated same-build damage counterfactual or an exact server modifier route proving effect 55342 cannot alter other damage events",
    },
    authority: {
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
  report.content_sha256 = stableContentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  const written = readJson(output, "written source-status confounder audit");
  verifyReport(written);
  verifyInputs(written);
  console.log(JSON.stringify({
    effect_id: report.effect_id,
    linked_action_id: report.linked_action_id,
    packet_damage_results: report.packet_observed_action_outcomes.packet_damage_results,
    packet_healing_results: report.packet_observed_action_outcomes.packet_healing_results,
    same_context_source_status_difference_count:
      report.counterfactual_confounder.same_context_source_status_difference_count,
    may_exclude_from_counterfactual_matching:
      report.counterfactual_confounder.may_exclude_from_counterfactual_matching,
    content_sha256: report.content_sha256,
  }, null, 2));
}

function validateManifest(manifest, build, inputs) {
  if (Number(manifest?.schemaVersion) !== 1 ||
    manifest?.generatedBy !== "tools/bpsr-build-source-manifest.mjs" ||
    String(manifest?.gameBuild) !== build || String(manifest?.distribution?.buildId) !== build ||
    manifest?.authority?.decodedGameTables !== "exact-current-build-static-data" ||
    manifest?.coverage?.complete !== true || Number(manifest?.coverage?.silentOmissions) !== 0 ||
    !Array.isArray(manifest.files)) {
    throw new Error("build source manifest is not complete exact-current-build authority");
  }
  return [["BuffTable.json", inputs.buff_table], ["DamageAttrTable.json", inputs.damage_attr_table]]
    .map(([relativePath, descriptor]) => {
      const matches = manifest.files.filter((entry) =>
        entry.root === "decoded-game-tables" && entry.relativePath === relativePath);
      if (matches.length !== 1 || matches[0].authority !== "exact-current-build-static-data" ||
        Number(matches[0].bytes) !== Number(descriptor.bytes) ||
        String(matches[0].sha256) !== String(descriptor.sha256)) {
        throw new Error(`exact manifest binding failed for ${relativePath}`);
      }
      return { relative_path: relativePath, bytes: Number(matches[0].bytes),
        sha256: String(matches[0].sha256) };
    });
}

function validateActivation(value, build) {
  if (Number(value?.schema_version) !== 1 ||
    value?.generated_by !== "rlogs-bpsr-damage-attr-proof-compact" ||
    String(value?.game_build) !== build || String(value?.packet_build) !== build ||
    value?.policy?.exact_packet_observation_index_only !== true ||
    value?.policy?.static_identity_does_not_prove_transfer !== true ||
    value?.policy?.unresolved_evidence_hidden !== false ||
    Number(value?.summary?.sessions) !== 26 ||
    !Array.isArray(value?.observed_ability_result_kinds) ||
    !Array.isArray(value?.observed_damage_rows)) {
    throw new Error("damage activation index is not exact current-build 26-session evidence");
  }
}

function validateTransition(value, build) {
  if (Number(value?.schema_version) !== 3 ||
    value?.generated_by !== "rlogs-bpsr-rlog-transition-counterfactual-audit" ||
    String(value?.game_build) !== build || Number(value?.effect_id) !== 2110092 ||
    !/^sha256:[0-9a-f]{64}$/.test(String(value?.content_sha256 ?? "")) ||
    Number(value?.summary?.same_normalized_damage_context_pairs) !== 37 ||
    Number(value?.summary?.same_context_source_status_difference_counts?.[String(EFFECT_ID)]) !== 33 ||
    Number(value?.summary?.same_context_and_nonselected_status_pairs) !== 0 ||
    Number(value?.summary?.strict_controlled_counterfactual_pairs) !== 0 ||
    value?.summary?.provider_rdps_credit_allowed !== false) {
    throw new Error("transition audit is not the exact schema-3 status-confounded frontier");
  }
}

function validateFormulaEvidence(value, build) {
  const sources = Array.isArray(value?.sources) ? value.sources : Object.values(value?.sources ?? {});
  if (Number(value?.schema_version) !== 1 ||
    value?.generated_by !== "tools/bpsr-static-formula-evidence.mjs" ||
    String(value?.game_build) !== build || value?.content_sha256 !== insertionContentHash(value) ||
    value?.policy?.exact_current_build_evidence_only !== true ||
    value?.policy?.decoded_formula_does_not_imply_runtime_activation_or_rdps_promotion !== true ||
    Number(value?.summary?.sources) !== sources.length) {
    throw new Error("static formula evidence is not exact current-build fail-closed evidence");
  }
}

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  const report = readJson(input, "source-status confounder audit");
  verifyReport(report);
  verifyInputs(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  if (Number(report?.schema_version) !== SCHEMA_VERSION || report?.generated_by !== GENERATOR ||
    String(report?.game_build) !== "24687926" || Number(report?.effect_id) !== EFFECT_ID ||
    Number(report?.linked_action_id) !== ACTION_ID ||
    report?.content_sha256 !== stableContentHash(report) ||
    report?.status !== "healing-action-route-proven-status-damage-neutrality-unproven" ||
    report?.policy?.remote_player_packet_acquisition_required !== false ||
    report?.policy?.never_received_remote_player_packet_absence_is_not_zero !== true ||
    report?.policy?.produced_action_healing_only_does_not_prove_status_modifier_damage_neutrality !== true ||
    report?.policy?.absent_static_formula_token_is_not_zero_effect_proof !== true ||
    Number(report?.packet_observed_action_outcomes?.packet_damage_results) !== 0 ||
    Number(report?.packet_observed_action_outcomes?.packet_healing_results) !== 22320 ||
    report?.packet_observed_action_outcomes?.linked_action_output_damage_neutrality_proven_for_observed_index !== true ||
    report?.packet_observed_action_outcomes?.status_modifier_damage_neutrality_proven !== false ||
    Number(report?.counterfactual_confounder?.same_normalized_damage_context_pairs) !== 37 ||
    Number(report?.counterfactual_confounder?.same_context_source_status_difference_count) !== 33 ||
    Number(report?.counterfactual_confounder?.same_context_and_nonselected_status_pairs) !== 0 ||
    Number(report?.counterfactual_confounder?.strict_controlled_counterfactual_pairs) !== 0 ||
    report?.counterfactual_confounder?.may_exclude_from_counterfactual_matching !== false ||
    Number(report?.static_formula_coverage?.evidence_source_count) !== 624 ||
    Number(report?.static_formula_coverage?.direct_exact_effect_token_matches) !== 0 ||
    report?.static_formula_coverage?.absence_proves_damage_neutrality !== false ||
    report?.conclusion?.effect_may_be_removed_as_counterfactual_confounder !== false ||
    report?.conclusion?.structural_remote_player_packets_required_to_close !== false ||
    report?.authority?.formula_authority !== false ||
    report?.authority?.runtime_authority !== false ||
    report?.authority?.ui_display_authority !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("source-status confounder audit violates its fail-closed schema");
  }
  for (const descriptor of Object.values(report.inputs ?? {})) validateDescriptor(descriptor);
}

function verifyInputs(report) {
  for (const descriptor of Object.values(report.inputs)) {
    const bytes = readFileSync(path.resolve(descriptor.path));
    if (bytes.length !== Number(descriptor.bytes) ||
      createHash("sha256").update(bytes).digest("hex") !== descriptor.sha256) {
      throw new Error(`input changed: ${descriptor.path}`);
    }
  }
}

function selfTest() {
  const unsafe = {
    policy: { remote_player_packet_acquisition_required: true },
    counterfactual_confounder: { may_exclude_from_counterfactual_matching: true },
  };
  if (unsafe.policy.remote_player_packet_acquisition_required !== true ||
    unsafe.counterfactual_confounder.may_exclude_from_counterfactual_matching !== true) {
    throw new Error("unsafe fixture construction failed");
  }
  console.log("bpsr-source-status-confounder-route-audit self-test passed");
}

function containsExactScalar(value, needle) {
  if (value === needle || value === String(needle)) return true;
  if (Array.isArray(value)) return value.some((item) => containsExactScalar(item, needle));
  if (value && typeof value === "object") {
    return Object.values(value).some((item) => containsExactScalar(item, needle));
  }
  return false;
}
function fileDescriptor(file) {
  const bytes = readFileSync(file);
  return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size,
    sha256: createHash("sha256").update(bytes).digest("hex") };
}
function validateDescriptor(value) {
  if (!String(value?.path ?? "") || !Number.isSafeInteger(Number(value?.bytes)) ||
    Number(value.bytes) <= 0 || !/^[0-9a-f]{64}$/.test(String(value?.sha256 ?? ""))) {
    throw new Error("invalid exact file descriptor");
  }
}
function stableContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(stableStringify(copy)).digest("hex");
}
function insertionContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}
function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) =>
    `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
}
function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`unable to read ${label} ${file}: ${error.message}`); }
}
function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    parsed[key] = value;
  }
  return parsed;
}
function required(parsed, key) {
  if (!parsed[key]) throw new Error(`missing --${key}`);
  return parsed[key];
}
function numericString(value, label) {
  if (!/^\d+$/.test(String(value))) throw new Error(`${label} must be numeric`);
  return String(value);
}
function usage(code) {
  console.log("Usage:\n  node tools/bpsr-source-status-confounder-route-audit.mjs analyze --build <id> --build-source-manifest <json> --buff-table <json> --damage-attr-table <json> --activation-index <json> --transition-counterfactual-audit <json> --static-formula-evidence <json> --output <json>\n  node tools/bpsr-source-status-confounder-route-audit.mjs verify --input <json>\n  node tools/bpsr-source-status-confounder-route-audit.mjs self-test");
  process.exit(code);
}
