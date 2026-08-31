#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR = "tools/bpsr-effect-fight-attribute-scope-proof.mjs";
const COMPONENT_FIELDS = ["AttrFinal", "AttrTotal", "AttrAdd", "AttrExAdd", "AttrPer", "AttrExPer"];
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "analyze") analyze(options);
else if (command === "verify") verifyFile(resolvePath(required(options, "input")), true);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(args) {
  const build = required(args, "build");
  const effectId = integer(required(args, "effect"), "effect");
  const files = {
    attribute_proof: resolvePath(required(args, "attribute-proof")),
    defense_lifecycle_proof: resolvePath(required(args, "defense-lifecycle-proof")),
    fight_attr_table: resolvePath(required(args, "fight-attr-table")),
    build_source_manifest: resolvePath(required(args, "build-source-manifest")),
    target_mitigation_rollup: resolvePath(required(args, "target-mitigation-rollup")),
    preflight: resolvePath(required(args, "preflight")),
  };
  const output = resolvePath(required(args, "output"));
  const proof = buildProof({ build, effectId, files });
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(proof, null, 2)}\n`, "utf8");
  verifyFile(output, true);
  console.log(
    `Effect ${effectId} fight-attribute scope: ${proof.summary.selected_fight_attribute_components} components; `
      + `${proof.summary.proven_reversible_constant_components} reversible constant component; `
      + `hidden damage-stage exclusion=false.`,
  );
}

function buildProof({ build, effectId, files }) {
  assert(/^\d+$/.test(build), "build must contain only ASCII digits");
  assert(effectId === 2201452, "this focused proof only accepts exact effect 2201452");
  for (const [label, file] of Object.entries(files)) requireFile(file, label.replaceAll("_", " "));
  const source = readJson(files.attribute_proof, "all-component attribute proof");
  const defense = readJson(files.defense_lifecycle_proof, "defense lifecycle proof");
  const fightAttr = readJson(files.fight_attr_table, "FightAttrTable");
  const manifest = readJson(files.build_source_manifest, "complete build source manifest");
  const rollup = readJson(files.target_mitigation_rollup, "target mitigation rollup");
  const preflight = readJson(files.preflight, "build preflight");

  assert(source.schema_version === 26 && source.generated_by === "rlogs-bpsr-rdps-status-attribute-proof", "unsupported attribute proof");
  assert(source.policy?.formula_inference === false, "attribute proof changed its inference policy");
  assert(source.policy?.unresolved_evidence_is_hidden === false, "attribute proof hides unresolved evidence");
  assert(source.policy?.wire_message_state?.includes("same_capture_connection_and_stream"), "attribute proof lacks exact wire identity");
  assert((source.selected_effect_ids ?? []).map(Number).includes(effectId), "attribute proof lacks selected effect");
  assert((source.reported_effect_ids ?? []).map(Number).includes(effectId), "attribute proof does not report selected effect");
  assert(defense.schema_version === 2 && defense.generated_by === "tools/bpsr-defense-percent-lifecycle-proof.mjs", "unsupported defense lifecycle proof");
  assert(String(defense.game_build) === build && Number(defense.effect_id) === effectId, "defense lifecycle proof identity mismatch");
  assert(defense.content_sha256 === hashJsonWithoutContentHash(defense), "defense lifecycle proof content hash changed");
  assert(Number(defense.packet_raw_percent_proof?.raw_percent_attribute_id) === 11354, "defense proof raw-percent attribute changed");
  assert(Number(defense.packet_raw_percent_proof?.joined_exact_single_effect_occurrences) === 47, "defense proof raw-percent join count changed");
  assert(Number(defense.packet_raw_percent_proof?.unresolved_final_only_occurrences) === 4, "defense proof final-only count changed");
  assert(defense.summary?.provider_rdps_credit_allowed === false, "defense proof improperly grants credit");
  assert(String(manifest.gameBuild ?? manifest.game_build ?? "") === build, "build manifest identity mismatch");
  assert(String(rollup.game_build ?? "") === build, "target mitigation rollup identity mismatch");
  assert(String(preflight.game_build ?? "") === build, "preflight identity mismatch");
  assert(preflight.ready_for_snapshot === false && preflight.runtime_promotion_allowed === false, "scope proof cannot consume a promoted preflight");

  const tableManifest = (manifest.files ?? []).find((entry) => entry.id === "decoded-game-tables:FightAttrTable.json");
  assert(tableManifest?.authority === "exact-current-build-static-data", "FightAttrTable lacks exact-build manifest authority");
  assert(Number(tableManifest.bytes) === statSync(files.fight_attr_table).size, "FightAttrTable manifest byte length changed");
  assert(String(tableManifest.sha256) === sha256Hex(files.fight_attr_table), "FightAttrTable manifest hash changed");

  const rows = Object.values(fightAttr);
  assert(rows.length > 0, "FightAttrTable has no rows");
  const componentIds = [...new Set(rows.flatMap((row) => COMPONENT_FIELDS.map((field) => Number(row[field])))
    .filter((value) => Number.isSafeInteger(value) && value > 0))].sort((a, b) => a - b);
  const selectedIds = [...new Set((source.selected_attribute_ids ?? []).map(Number))].sort((a, b) => a - b);
  assert(JSON.stringify(selectedIds) === JSON.stringify(componentIds), "attribute proof does not cover every FightAttrTable component exactly once");

  const rollupRlogs = new Set((rollup.runs ?? []).flatMap((run) => run.cohort?.source_inputs ?? []).map(normalizePath));
  const sourceRlogs = (source.sessions ?? []).map((session) => normalizePath(session.rlog));
  assert(sourceRlogs.length > 0, "attribute proof has no sessions");
  const unboundRlogs = sourceRlogs.filter((rlog) => !rollupRlogs.has(rlog));
  assert(unboundRlogs.length === 0, `attribute proof sessions are absent from exact-build rollup: ${unboundRlogs.join(", ")}`);

  const metadata = new Map();
  for (const row of rows) {
    for (const field of COMPONENT_FIELDS) {
      const attributeId = Number(row[field]);
      if (attributeId > 0) metadata.set(attributeId, {
        fight_attribute_axis_id: Number(row.Id),
        enum_name: String(row.EnumName ?? ""),
        official_name_evidence_only: String(row.OfficialName ?? ""),
        component_field: field,
      });
    }
  }

  const correlations = [];
  let equationSystemsWithReportedEffect = 0;
  for (const system of source.wire_additive_equation_systems ?? []) {
    if (Number(system.equations_containing_reported_effect ?? 0) > 0) equationSystemsWithReportedEffect += 1;
    const equations = (system.equations ?? []).filter((equation) =>
      equation.terms?.length === 1 && Number(equation.terms[0].effect_id) === effectId,
    );
    if (equations.length === 0) continue;
    const sessions = new Set();
    const signs = new Set();
    const normalizedCoefficients = new Set();
    const deltas = [];
    let occurrences = 0;
    for (const equation of equations) {
      const sign = Number(equation.terms[0].signed_presence_delta);
      assert(sign === 1 || sign === -1, "single-effect equation has a non-binary transition");
      const examples = Array.isArray(equation.examples) ? equation.examples : [equation.examples].filter(Boolean);
      assert(examples.length === Number(equation.count), "attribute proof examples are truncated");
      const delta = Number(equation.raw_attribute_delta);
      signs.add(sign);
      normalizedCoefficients.add(delta / sign);
      occurrences += Number(equation.count);
      deltas.push({ signed_presence_delta: sign, raw_attribute_delta: delta, occurrences: Number(equation.count) });
      for (const example of examples) sessions.add(String(example.session_id));
    }
    correlations.push({
      attribute_id: Number(system.attribute_id),
      ...(metadata.get(Number(system.attribute_id)) ?? {}),
      exact_single_effect_occurrences: occurrences,
      independent_sessions: sessions.size,
      observed_presence_signs: [...signs].sort((a, b) => a - b),
      normalized_coefficients: [...normalizedCoefficients].sort((a, b) => a - b),
      same_wire_deltas: deltas.sort((a, b) => a.signed_presence_delta - b.signed_presence_delta
        || a.raw_attribute_delta - b.raw_attribute_delta),
      bidirectional: signs.size === 2,
      constant_normalized_coefficient: normalizedCoefficients.size === 1,
    });
  }
  correlations.sort((left, right) => right.exact_single_effect_occurrences - left.exact_single_effect_occurrences
    || left.attribute_id - right.attribute_id);
  const classified = classifyCorrelations(correlations);
  assert(classified.proven.length === 1, "expected exactly one reversible constant component");
  const proven = classified.proven[0];
  assert(proven.attribute_id === 11354 && proven.component_field === "AttrPer", "reversible component is not raw Armor percent 11354");
  assert(JSON.stringify(proven.normalized_coefficients) === JSON.stringify([1000]), "raw Armor percent coefficient changed");
  assert(proven.exact_single_effect_occurrences === 47 && proven.independent_sessions === 13, "raw Armor percent witness coverage changed");
  assert(JSON.stringify(classified.oneDirection.map((row) => row.attribute_id).sort((a, b) => a - b))
    === JSON.stringify([11710, 11711, 11712]), "one-direction constant correlation inventory changed");

  const rawArmorTransitions = singleEffectExamples(source, 11354, effectId);
  const rawCritAddTransitions = singleEffectExamples(source, 11712, effectId);
  const critByWire = new Map(rawCritAddTransitions.map((row) => [transitionKey(row), row]));
  const rawArmorCritCooccurrence = rawArmorTransitions.map((row) => ({
    ...row,
    raw_crit_add_delta: critByWire.get(transitionKey(row))?.raw_attribute_delta ?? null,
  }));
  const armorApplications = rawArmorCritCooccurrence.filter((row) => row.state === "applied");
  const armorRemovals = rawArmorCritCooccurrence.filter((row) => row.state === "removed");
  const critApplications = armorApplications.filter((row) => row.raw_crit_add_delta !== null);
  const critRemovals = armorRemovals.filter((row) => row.raw_crit_add_delta !== null);
  assert(rawArmorTransitions.length === 47, "raw Armor transition coverage changed");
  assert(armorApplications.length === 26 && armorRemovals.length === 21, "raw Armor lifecycle direction counts changed");
  assert(critApplications.length === 0, "raw Crit add unexpectedly co-updates on application");
  assert(critRemovals.length === 2 && critRemovals.every((row) => row.raw_crit_add_delta === 50), "raw Crit removal co-update changed");

  const report = {
    schema_version: 2,
    generated_by: GENERATOR,
    generated_at: new Date().toISOString(),
    game_build: build,
    effect_id: effectId,
    policy: {
      exact_numeric_effect_attribute_and_build_identity_are_authoritative: true,
      localized_names_are_evidence_only: true,
      all_exact_build_fight_attribute_components_are_selected: true,
      same_wire_correlation_is_not_causation_without_reversible_constant_replay: true,
      one_direction_constant_correlations_remain_unresolved: true,
      sparse_one_direction_co_updates_do_not_establish_an_unconditional_component: true,
      nonstationary_correlations_remain_visible: true,
      absence_of_an_observed_fight_attribute_component_does_not_exclude_hidden_damage_logic: true,
      structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
      ordinary_damage_is_not_modified_by_this_proof: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: Object.fromEntries(Object.entries(files).map(([key, file]) => [key, fileIdentity(file)])),
    build_identity: {
      fight_attr_table_manifest_authority: String(tableManifest.authority),
      selected_fight_attribute_components: componentIds.length,
      exact_build_sessions: sourceRlogs.length,
      exact_build_sessions_bound_to_rollup: sourceRlogs.length,
      preflight_ready_for_snapshot: false,
      runtime_promotion_allowed: false,
    },
    proven_packet_observable_components: classified.proven,
    unresolved_one_direction_constant_correlations: classified.oneDirection,
    unresolved_nonstationary_same_wire_correlations: classified.nonstationary,
    raw_armor_to_crit_co_transition_test: {
      raw_armor_percent_attribute_id: 11354,
      raw_crit_add_attribute_id: 11712,
      exact_raw_armor_presence_transitions: rawArmorTransitions.length,
      raw_armor_applications: armorApplications.length,
      raw_armor_removals: armorRemovals.length,
      applications_with_raw_crit_add_update: critApplications.length,
      removals_with_raw_crit_add_update: critRemovals.length,
      exact_raw_armor_transitions_without_raw_crit_add_co_update:
        rawArmorCritCooccurrence.filter((row) => row.raw_crit_add_delta === null).length,
      observed_removal_only_raw_crit_add_delta: 50,
      unconditional_fixed_negative_50_raw_crit_add_component_supported: false,
      conditional_or_indirect_crit_behavior_excluded: false,
      co_update_witnesses: critRemovals,
    },
    conclusion: {
      only_proven_reversible_constant_fight_attribute_component_id: 11354,
      only_proven_reversible_constant_component_role: "FightAttrTable AttrPer for physical Armor axis 11350",
      raw_percent_basis_points_per_effect_presence: 1000,
      direct_raw_percent_identity_proven: true,
      effect_is_defense_stat_only_across_observed_fight_attribute_components_proven: false,
      hidden_damage_stage_behavior_excluded: false,
      exact_target_defense_to_damage_formula_proven: false,
      exact_damage_operation_order_proven: false,
      exact_damage_integer_rounding_proven: false,
      packet_damage_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    summary: {
      selected_fight_attribute_components: componentIds.length,
      equation_systems_with_reported_effect: equationSystemsWithReportedEffect,
      components_with_exact_single_effect_same_wire_correlations: correlations.length,
      proven_reversible_constant_components: classified.proven.length,
      unresolved_one_direction_constant_components: classified.oneDirection.length,
      unresolved_nonstationary_components: classified.nonstationary.length,
      exact_raw_armor_transitions_without_raw_crit_add_co_update:
        rawArmorCritCooccurrence.filter((row) => row.raw_crit_add_delta === null).length,
      raw_armor_percent_exact_occurrences: proven.exact_single_effect_occurrences,
      raw_armor_percent_independent_sessions: proven.independent_sessions,
      provider_rdps_credit_allowed: false,
      ui_display_authority: false,
    },
  };
  report.content_sha256 = hashJson(report);
  verifyProof(report, false);
  return report;
}

function classifyCorrelations(rows) {
  return {
    proven: rows.filter((row) => row.bidirectional && row.constant_normalized_coefficient && row.independent_sessions >= 2),
    oneDirection: rows.filter((row) => !row.bidirectional && row.constant_normalized_coefficient),
    nonstationary: rows.filter((row) => !row.constant_normalized_coefficient),
  };
}

function singleEffectExamples(source, attributeId, effectId) {
  const system = (source.wire_additive_equation_systems ?? [])
    .find((row) => Number(row.attribute_id) === attributeId);
  assert(system, `attribute proof lacks equation system ${attributeId}`);
  const rows = [];
  for (const equation of system.equations ?? []) {
    if (equation.terms?.length !== 1 || Number(equation.terms[0].effect_id) !== effectId) continue;
    const sign = Number(equation.terms[0].signed_presence_delta);
    const examples = Array.isArray(equation.examples) ? equation.examples : [equation.examples].filter(Boolean);
    assert(examples.length === Number(equation.count), `attribute ${attributeId} examples are truncated`);
    for (const example of examples) rows.push({
      session_id: String(example.session_id),
      wire_capture_sequence: Number(example.wire_capture_sequence),
      state: sign === 1 ? "applied" : "removed",
      raw_attribute_delta: Number(equation.raw_attribute_delta),
      instance_id: Number((example.status_instances ?? [])
        .find((status) => Number(status.effect_id) === effectId)?.instance_id),
    });
  }
  return rows;
}

function transitionKey(row) { return `${row.session_id}|${row.wire_capture_sequence}|${row.state}`; }

function verifyFile(file, verifyInputs) {
  const proof = readJson(file, "fight-attribute scope proof");
  verifyProof(proof, verifyInputs);
  console.log(`Verified effect ${proof.effect_id} fight-attribute scope; provider credit allowed=false.`);
}

function verifyProof(proof, verifyInputs) {
  assert([1, 2].includes(proof.schema_version) && proof.generated_by === GENERATOR, "unsupported scope proof schema or generator");
  assert(/^\d+$/.test(String(proof.game_build ?? "")) && Number(proof.effect_id) === 2201452, "scope proof identity changed");
  assert(proof.policy?.all_exact_build_fight_attribute_components_are_selected === true, "scope proof lost complete component selection");
  assert(proof.policy?.same_wire_correlation_is_not_causation_without_reversible_constant_replay === true, "scope proof correlation policy changed");
  assert(proof.policy?.one_direction_constant_correlations_remain_unresolved === true, "scope proof hides one-direction correlations");
  assert(proof.policy?.absence_of_an_observed_fight_attribute_component_does_not_exclude_hidden_damage_logic === true, "scope proof overclaims hidden logic exclusion");
  assert(proof.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements === true, "scope proof remote packet policy changed");
  assert(proof.summary?.selected_fight_attribute_components === 906, "scope proof component count changed");
  assert(proof.summary?.components_with_exact_single_effect_same_wire_correlations === 26, "scope proof correlation count changed");
  assert(proof.summary?.proven_reversible_constant_components === 1, "scope proof reversible count changed");
  assert(proof.summary?.unresolved_one_direction_constant_components === 3, "scope proof one-direction count changed");
  assert(proof.summary?.unresolved_nonstationary_components === 22, "scope proof nonstationary count changed");
  const proven = proof.proven_packet_observable_components?.[0];
  assert(proven?.attribute_id === 11354 && proven?.component_field === "AttrPer", "scope proof raw Armor component changed");
  assert(JSON.stringify(proven?.normalized_coefficients) === JSON.stringify([1000]), "scope proof raw Armor coefficient changed");
  assert(proven?.exact_single_effect_occurrences === 47 && proven?.independent_sessions === 13, "scope proof raw Armor coverage changed");
  assert(JSON.stringify((proof.unresolved_one_direction_constant_correlations ?? []).map((row) => row.attribute_id).sort((a, b) => a - b))
    === JSON.stringify([11710, 11711, 11712]), "scope proof lost one-direction correlations");
  assert(proof.conclusion?.direct_raw_percent_identity_proven === true, "scope proof lost raw-percent identity");
  assert(proof.conclusion?.effect_is_defense_stat_only_across_observed_fight_attribute_components_proven === false, "scope proof overclaims defense-only behavior");
  assert(proof.conclusion?.hidden_damage_stage_behavior_excluded === false, "scope proof overclaims hidden damage exclusion");
  assert(proof.conclusion?.formula_authority === false && proof.conclusion?.runtime_authority === false, "scope proof improperly grants authority");
  assert(proof.conclusion?.ui_display_authority === false && proof.conclusion?.provider_rdps_credit_allowed === false, "scope proof improperly grants UI or credit authority");
  if (proof.schema_version === 2) {
    const co = proof.raw_armor_to_crit_co_transition_test;
    assert(proof.policy?.sparse_one_direction_co_updates_do_not_establish_an_unconditional_component === true, "schema 2 sparse co-update policy changed");
    assert(co?.raw_armor_percent_attribute_id === 11354 && co?.raw_crit_add_attribute_id === 11712, "schema 2 co-transition attributes changed");
    assert(co?.exact_raw_armor_presence_transitions === 47, "schema 2 raw Armor transition count changed");
    assert(co?.raw_armor_applications === 26 && co?.raw_armor_removals === 21, "schema 2 raw Armor directions changed");
    assert(co?.applications_with_raw_crit_add_update === 0 && co?.removals_with_raw_crit_add_update === 2, "schema 2 Crit co-update counts changed");
    assert(co?.exact_raw_armor_transitions_without_raw_crit_add_co_update === 45, "schema 2 missing Crit co-update count changed");
    assert(co?.observed_removal_only_raw_crit_add_delta === 50, "schema 2 sparse Crit delta changed");
    assert(co?.unconditional_fixed_negative_50_raw_crit_add_component_supported === false, "schema 2 improperly accepts unconditional Crit component");
    assert(co?.conditional_or_indirect_crit_behavior_excluded === false, "schema 2 overclaims Crit exclusion");
    assert(co?.co_update_witnesses?.length === 2, "schema 2 Crit witnesses changed");
    assert(proof.summary?.exact_raw_armor_transitions_without_raw_crit_add_co_update === 45, "schema 2 co-update summary changed");
  }
  const { content_sha256: recordedHash, ...withoutHash } = proof;
  assert(recordedHash === hashJson(withoutHash), "scope proof content hash is invalid");
  if (verifyInputs) {
    for (const [label, input] of Object.entries(proof.inputs ?? {})) {
      const inputFile = resolvePath(input.path);
      requireFile(inputFile, label.replaceAll("_", " "));
      assert(statSync(inputFile).size === input.bytes, `${label} byte length changed`);
      assert(sha256(inputFile) === input.sha256, `${label} content hash changed`);
    }
  }
}

function selfTest() {
  const rows = [
    { attribute_id: 1, bidirectional: true, constant_normalized_coefficient: true, independent_sessions: 2 },
    { attribute_id: 2, bidirectional: false, constant_normalized_coefficient: true, independent_sessions: 2 },
    { attribute_id: 3, bidirectional: true, constant_normalized_coefficient: false, independent_sessions: 2 },
  ];
  const classified = classifyCorrelations(rows);
  assert(classified.proven.length === 1 && classified.proven[0].attribute_id === 1, "self-test lost reversible proof");
  assert(classified.oneDirection.length === 1 && classified.oneDirection[0].attribute_id === 2, "self-test lost one-direction evidence");
  assert(classified.nonstationary.length === 1 && classified.nonstationary[0].attribute_id === 3, "self-test lost nonstationary evidence");
  console.log("bpsr-effect-fight-attribute-scope-proof self-test passed.");
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`unexpected argument: ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function required(args, key) { if (!args[key]) throw new Error(`missing required --${key}`); return args[key]; }
function integer(value, label) { const parsed = Number(value); if (!/^\d+$/.test(String(value)) || !Number.isSafeInteger(parsed)) throw new Error(`${label} must be a safe ASCII integer`); return parsed; }
function resolvePath(value) { return path.isAbsolute(value) ? value : path.resolve(repoRoot, value); }
function normalizePath(value) { return String(value).replaceAll("\\", "/"); }
function relativePath(file) { return normalizePath(path.relative(repoRoot, file)); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`missing ${label}: ${file}`); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`invalid ${label} JSON at ${file}: ${error.message}`); } }
function fileIdentity(file) { return { path: relativePath(file), bytes: statSync(file).size, sha256: sha256(file) }; }
function sha256Hex(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function sha256(file) { return `sha256:${sha256Hex(file)}`; }
function hashJson(value) { return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`; }
function hashJsonWithoutContentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return hashJson(copy); }
function assert(condition, message) { if (!condition) throw new Error(message); }

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-effect-fight-attribute-scope-proof.mjs analyze --build <id> --effect 2201452 \\
    --attribute-proof <json> --defense-lifecycle-proof <json> --fight-attr-table <json> \\
    --build-source-manifest <json> --target-mitigation-rollup <json> --preflight <json> --output <json>
  node tools/bpsr-effect-fight-attribute-scope-proof.mjs verify --input <json>
  node tools/bpsr-effect-fight-attribute-scope-proof.mjs self-test`);
  process.exit(exitCode);
}
