#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-target-status-action-route-audit.mjs";
const EFFECT_IDS = [9903, 55230, 55301, 55302, 55339, 55361, 600001, 823224, 823226,
  2110093, 2201452, 2203182];

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
  const nearPairPath = path.resolve(required(parsed, "near-pair-proof"));
  const output = path.resolve(required(parsed, "output"));
  const manifest = readJson(manifestPath, "build source manifest");
  const buffTable = readJson(buffTablePath, "BuffTable");
  const damageAttrTable = readJson(damageAttrTablePath, "DamageAttrTable");
  const activation = readJson(activationPath, "damage action activation index");
  const nearPair = readJson(nearPairPath, "near-pair proof");
  const inputs = {
    build_source_manifest: fileDescriptor(manifestPath),
    buff_table: fileDescriptor(buffTablePath),
    damage_attr_table: fileDescriptor(damageAttrTablePath),
    activation_index: fileDescriptor(activationPath),
    near_pair_proof: fileDescriptor(nearPairPath),
  };
  const tableBindings = validateManifest(manifest, build, inputs);
  validateActivation(activation, build);
  validateNearPair(nearPair, build);

  const candidateIds = new Set(nearPair.confounders.status_effect_ids.map(Number));
  const witnesses = new Map(nearPair.confounders.same_axis_status_invariance
    .status_effect_witnesses.map((row) => [Number(row.effect_id), row]));
  const rows = EFFECT_IDS.map((effectId) => {
    const buff = buffTable[String(effectId)];
    if (!buff || Number(buff.Id) !== effectId) throw new Error(`missing exact BuffTable row ${effectId}`);
    const ability = activation.observed_ability_result_kinds.find(
      (row) => Number(row.ability_id) === effectId,
    ) ?? null;
    const damageRows = activation.observed_damage_rows.filter(
      (row) => Number(row.type_enum) === effectId,
    ).map((row) => {
      const exact = damageAttrTable[String(row.damage_id)];
      if (!exact || stableStringify(exact) !== stableStringify(row.semantic_row)) {
        throw new Error(`activation row ${row.damage_id} does not match exact DamageAttrTable data`);
      }
      return {
        damage_attr_id: Number(row.damage_id),
        damage_script: row.damage_script === null ? null : String(row.damage_script),
        packet_damage_results: Number(row.packet_damage_results),
        packet_healing_results: Number(row.packet_healing_results),
      };
    });
    const packetDamageResults = Number(ability?.packet_damage_results ?? 0);
    const packetHealingResults = Number(ability?.packet_healing_results ?? 0);
    const producedActionDisposition = packetDamageResults > 0
      ? "produced-damage-action-observed"
      : packetHealingResults > 0
        ? "produced-action-healing-only-observed"
        : "no-produced-action-observed-in-26-session-index";
    return {
      effect_id: effectId,
      exact_buff_static_fields: {
        level: Number(buff.Level),
        repeat_add_rule: structuredClone(buff.RepeatAddRule ?? []),
        destroy_param: structuredClone(buff.DestroyParam ?? []),
        special_attr: structuredClone(buff.SpecialAttr ?? []),
        skill_id: Number(buff.SkillId ?? 0),
        is_client_buff: Boolean(buff.IsClientBuff),
      },
      semantic_name_design_evidence_only: String(buff.NameDesign ?? ""),
      produced_action_observation: {
        packet_damage_results: packetDamageResults,
        packet_healing_results: packetHealingResults,
        damage_attr_rows: damageRows,
        disposition: producedActionDisposition,
      },
      near_pair_candidate_status_delta: candidateIds.has(effectId),
      same_axis_status_witness: witnesses.has(effectId)
        ? structuredClone(witnesses.get(effectId))
        : null,
      status_modifier_damage_neutrality_proven: false,
      may_eliminate_from_target_status_delta: false,
      formula_authority: false,
      provider_rdps_credit_allowed: false,
    };
  });
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    status: "exact-produced-action-routes-audited-status-modifier-neutrality-unproven",
    policy: {
      exact_numeric_effect_ids_build_and_input_hashes_are_authoritative: true,
      localized_names_and_name_design_are_semantic_evidence_only: true,
      produced_action_healing_only_does_not_prove_status_modifier_damage_neutrality: true,
      no_observed_produced_action_is_not_zero_effect_proof: true,
      same_axis_equal_outcome_is_local_evidence_not_global_neutrality: true,
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
    effect_rows: rows,
    summary: {
      audited_effects: rows.length,
      produced_damage_action_effects: rows.filter((row) =>
        row.produced_action_observation.packet_damage_results > 0).length,
      produced_action_healing_only_effects: rows.filter((row) =>
        row.produced_action_observation.disposition ===
          "produced-action-healing-only-observed").length,
      no_produced_action_observed_effects: rows.filter((row) =>
        row.produced_action_observation.disposition ===
          "no-produced-action-observed-in-26-session-index").length,
      effects_eliminated_as_damage_neutral: 0,
      candidate_near_pair_status_effects_without_same_axis_witness:
        structuredClone(nearPair.confounders.same_axis_status_invariance
          .candidate_status_effect_ids_without_same_axis_witness),
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    conclusion: {
      exact_effect_55301_produced_action_packet_evidence:
        "3704 healing results and zero damage results across the exact 26-session activation index",
      exact_effect_2201452_produced_action_packet_evidence:
        "no produced damage or healing action observed in the exact 26-session activation index",
      candidate_near_pair_status_confounders_eliminated: false,
      reason:
        "produced-action routing and empty static SpecialAttr fields do not prove absence of server-side status modifiers on other damage events",
    },
    authority: {
      status_modifier_damage_neutrality_proven: false,
      target_status_confounders_eliminated: false,
      exact_target_mitigation_formula_proven: false,
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
  const written = readJson(output, "written target-status action-route audit");
  verifyReport(written);
  verifyInputs(written);
  console.log(JSON.stringify(report.summary, null, 2));
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
      return {
        relative_path: relativePath,
        bytes: Number(matches[0].bytes),
        sha256: String(matches[0].sha256),
      };
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

function validateNearPair(value, build) {
  if (Number(value?.schema_version) !== 3 ||
    value?.generated_by !== "tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs" ||
    String(value?.game_build) !== build || value?.content_sha256 !== stableContentHash(value) ||
    JSON.stringify(value?.confounders?.same_axis_status_invariance
      ?.candidate_status_effect_ids_without_same_axis_witness) !==
      JSON.stringify([55301, 2201452]) ||
    value?.confounders?.same_axis_status_invariance
      ?.target_status_can_change_damage_outside_raw_defense !== true ||
    value?.authority?.formula_authority !== false ||
    value?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("near-pair proof is not the exact schema-3 status-confounded frontier");
  }
}

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  const report = readJson(input, "target-status action-route audit");
  verifyReport(report);
  verifyInputs(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  const rows = report?.effect_rows;
  const byId = new Map((rows ?? []).map((row) => [Number(row.effect_id), row]));
  if (Number(report?.schema_version) !== SCHEMA_VERSION || report?.generated_by !== GENERATOR ||
    String(report?.game_build) !== "24687926" || report?.content_sha256 !== stableContentHash(report) ||
    report?.status !== "exact-produced-action-routes-audited-status-modifier-neutrality-unproven" ||
    report?.policy?.produced_action_healing_only_does_not_prove_status_modifier_damage_neutrality !== true ||
    report?.policy?.no_observed_produced_action_is_not_zero_effect_proof !== true ||
    report?.policy?.unknown_and_unresolved_status_roles_are_preserved !== true ||
    report?.policy?.formula_authority !== false || report?.policy?.ui_display_authority !== false ||
    !Array.isArray(rows) || JSON.stringify(rows.map((row) => Number(row.effect_id))) !==
      JSON.stringify(EFFECT_IDS) ||
    rows.some((row) => row.status_modifier_damage_neutrality_proven !== false ||
      row.may_eliminate_from_target_status_delta !== false || row.formula_authority !== false ||
      row.provider_rdps_credit_allowed !== false) ||
    Number(byId.get(55301)?.produced_action_observation?.packet_damage_results) !== 0 ||
    Number(byId.get(55301)?.produced_action_observation?.packet_healing_results) !== 3704 ||
    byId.get(55301)?.produced_action_observation?.disposition !==
      "produced-action-healing-only-observed" ||
    Number(byId.get(2201452)?.produced_action_observation?.packet_damage_results) !== 0 ||
    Number(byId.get(2201452)?.produced_action_observation?.packet_healing_results) !== 0 ||
    Number(report?.summary?.audited_effects) !== 12 ||
    Number(report?.summary?.produced_damage_action_effects) !== 0 ||
    Number(report?.summary?.produced_action_healing_only_effects) !== 3 ||
    Number(report?.summary?.no_produced_action_observed_effects) !== 9 ||
    Number(report?.summary?.effects_eliminated_as_damage_neutral) !== 0 ||
    JSON.stringify(report?.summary?.candidate_near_pair_status_effects_without_same_axis_witness) !==
      JSON.stringify([55301, 2201452]) ||
    report?.conclusion?.candidate_near_pair_status_confounders_eliminated !== false ||
    report?.authority?.status_modifier_damage_neutrality_proven !== false ||
    report?.authority?.target_status_confounders_eliminated !== false ||
    report?.authority?.exact_target_mitigation_formula_proven !== false ||
    report?.authority?.formula_authority !== false || report?.authority?.runtime_authority !== false ||
    report?.authority?.ui_display_authority !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("target-status action-route audit violates its fail-closed schema");
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
  const fixture = { effect_rows: EFFECT_IDS.map((effect_id) => ({ effect_id })) };
  if (JSON.stringify(fixture.effect_rows.map((row) => row.effect_id)) !== JSON.stringify(EFFECT_IDS)) {
    throw new Error("exact effect inventory self-test failed");
  }
  console.log("bpsr-target-status-action-route-audit self-test passed");
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
  console.log("Usage:\n  node tools/bpsr-target-status-action-route-audit.mjs analyze --build <id> --build-source-manifest <json> --buff-table <json> --damage-attr-table <json> --activation-index <json> --near-pair-proof <json> --output <json>\n  node tools/bpsr-target-status-action-route-audit.mjs verify --input <json>\n  node tools/bpsr-target-status-action-route-audit.mjs self-test");
  process.exit(code);
}
