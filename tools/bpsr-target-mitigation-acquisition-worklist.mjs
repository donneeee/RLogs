#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 2;
const GENERATOR = "tools/bpsr-target-mitigation-acquisition-worklist.mjs";
const SUPPORTED_BLADE_SWEEP_SCALAR_SCHEMAS = new Set([8, 9, 10, 11]);
const DIAGNOSTIC_GENERATOR =
  "rlogs-bpsr-target-mitigation-transform-proof:target-status-relaxed-diagnostic";
const AXES = new Map([
  ["physical_defense", 11350], ["magic_defense", 11360], ["refined_defense", 11420],
  ["general_element_defense", 13200], ["fire_element_defense", 13210],
  ["water_element_defense", 13220], ["wood_element_defense", 13230],
  ["electric_element_defense", 13240], ["wind_element_defense", 13250],
  ["rock_element_defense", 13260], ["light_element_defense", 13270],
  ["dark_element_defense", 13280],
]);

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "analyze") analyze(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const effectId = positiveInteger(required(parsed, "effect"), "effect");
  const scalarPath = path.resolve(required(parsed, "blade-sweep-scalar-proof"));
  const offlinePath = path.resolve(required(parsed, "offline-exhaustion-proof"));
  const sameAxisProofPath = path.resolve(required(parsed, "same-axis-status-proof"));
  const diagnosticPaths = requiredMany(parsed, "diagnostic").map((value) => path.resolve(value));
  const output = path.resolve(required(parsed, "output"));
  const scalar = readJson(scalarPath, "Blade Sweep scalar proof");
  const offline = readJson(offlinePath, "offline exhaustion proof");
  const sameAxisProof = readJson(sameAxisProofPath, "same-axis status proof");
  validateScalar(scalar, build, effectId);
  validateOffline(offline, build);
  validateSameAxisProof(sameAxisProof, build, effectId);
  const diagnostics = diagnosticPaths.map((file) => {
    const value = readJson(file, "target-status-relaxed diagnostic");
    return validateDiagnostic(value, build, effectId, fileDescriptor(file));
  });
  const sourceRlogs = diagnostics.flatMap((entry) => entry.source_rlogs).sort(compareText);
  if (new Set(sourceRlogs).size !== sourceRlogs.length) {
    throw new Error("target mitigation diagnostics contain duplicate source RLOGs");
  }
  const expectedRlogs = [...scalar.target_mitigation_evidence.source_rlogs].sort(compareText);
  if (JSON.stringify(sourceRlogs) !== JSON.stringify(expectedRlogs)) {
    throw new Error("target mitigation diagnostics do not exactly cover the focused Blade Sweep cohort");
  }
  const axisRows = [...AXES].map(([axis, attributeId]) => {
    const rows = diagnostics.map((entry) => entry.axes[axis]);
    return {
      axis,
      current_attribute_id: attributeId,
      required_packet_property: rows[0].required_packet_property,
      samples_with_axis: sum(rows, (row) => row.counters.samples_with_axis),
      target_status_relaxed_distinct_axis_pairs:
        sum(rows, (row) => row.counters.distinct_axis_pairs),
      target_status_relaxed_deterministic_pairs:
        sum(rows, (row) => row.counters.deterministic_pairs),
      target_status_relaxed_divergent_output_pairs:
        sum(rows, (row) => row.counters.divergent_output_pairs),
      pairs_with_effect_in_target_status_delta:
        sum(rows, (row) => row.counters.pairs_with_selected_effect_in_status_delta),
      pairs_with_only_effect_in_target_status_delta:
        sum(rows, (row) => row.counters.pairs_with_only_selected_effect_in_status_delta),
      selected_effect_examples: rows.flatMap((row) => row.selected_effect_examples).slice(0, 12),
      controlled_formula_authority: false,
    };
  });
  const damageSamples = sum(diagnostics, (entry) => entry.sample_count);
  const auditedAxisSamples = sum(axisRows, (row) => row.samples_with_axis);
  if (damageSamples !== Number(scalar.target_mitigation_evidence.damage_samples) ||
    auditedAxisSamples !== Number(scalar.target_mitigation_evidence.audited_axis_samples)) {
    throw new Error("target mitigation diagnostic sample counts do not reconcile with the focused rollup");
  }
  const relaxedPairs = sum(axisRows, (row) => row.target_status_relaxed_distinct_axis_pairs);
  const effectPairs = sum(axisRows, (row) => row.pairs_with_effect_in_target_status_delta);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    effect_id: effectId,
    status: "acquisition-required-strict-controls-status-damage-relevance-observed",
    policy: {
      exact_numeric_effect_ids_and_build_are_authoritative: true,
      exact_input_hashes_are_embedded_and_verified: true,
      same_capture_only: true,
      cross_capture_pairing_allowed: false,
      target_status_relaxation_is_diagnostic_only: true,
      near_pair_is_not_controlled_counterfactual_proof: true,
      unknown_and_unresolved_evidence_is_preserved: true,
      structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
      provider_ownership_is_already_proven_from_observable_evidence: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      blade_sweep_scalar_proof: fileDescriptor(scalarPath),
      offline_exhaustion_proof: fileDescriptor(offlinePath),
      same_axis_status_proof: fileDescriptor(sameAxisProofPath),
      diagnostics: diagnosticPaths.map(fileDescriptor),
    },
    evidence_scope: {
      matching_build_capture_diagnostics: diagnostics.length,
      matching_build_source_rlogs: sourceRlogs,
      damage_samples: damageSamples,
      audited_axis_samples: auditedAxisSamples,
      maximum_measured_peak_working_set_bytes:
        Math.max(...diagnostics.map((entry) => entry.maximum_peak_bytes)),
      strict_controlled_groups: Number(scalar.target_mitigation_evidence.controlled_groups),
      target_status_relaxed_distinct_axis_pairs: relaxedPairs,
      pairs_with_effect_in_target_status_delta: effectPairs,
      global_same_axis_target_status_pairs:
        Number(sameAxisProof.confounders.same_axis_status_invariance
          .physical_defense_same_axis_status_pairs),
      global_same_axis_equal_output_pairs:
        Number(sameAxisProof.confounders.same_axis_status_invariance
          .physical_defense_same_axis_equal_output_pairs),
      global_same_axis_divergent_output_pairs:
        Number(sameAxisProof.confounders.same_axis_status_invariance
          .physical_defense_same_axis_divergent_output_pairs),
    },
    axes: axisRows,
    acquisition_contract: {
      target: "obtain a matching-build same-capture defense-axis counterfactual that survives the strict target-mitigation grouping contract",
      completed_prerequisites: [
        "exact effect 2110092 provider ownership is proven for every stable player-owned lifecycle event in the reviewed current-build cohort",
        "same-axis evidence proves target status can change damage independently of raw physical defense",
      ],
      required_controls: [
        "same session, run, source entity, direct source, target entity, ability, passive, hit identity, damage source/type, critical/lucky flags, and complete packet calculation identity",
        "same complete source status state and source attributes except volatile CurrentHP",
        "same complete target status state and target non-axis attributes except volatile CurrentHP",
        "different exact raw value in only the selected six-member mitigation attribute family",
        "one deterministic outcome per side and a divergent result for formula discrimination",
      ],
      blade_sweep_followup: [
        "after the generic armor-to-damage transform is proven, replay exact effect 2110092 present versus absent windows",
        "use canonical observable lifecycle and ownership evidence; do not require remote-player packet families the client never receives",
        "prove stacking, stage order, integer rounding, and party conservation before provider credit",
      ],
      forbidden_shortcuts: [
        "pairing across captures or runs",
        "treating target-status-relaxed near-pairs as controlled formula proof",
        "substituting a current character or target snapshot into an older event",
        "using localized names as runtime identity",
        "promoting the 6.5 percent static scalar as a final damage contribution formula",
        "making proof closure depend on remote-player packet families the client never receives",
      ],
      success_condition: "at least one exact divergent deterministic strict pair is observed and every candidate equation is checked for rejections, order, rounding, and conservation",
    },
    authority: {
      exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    blockers: [
      "the current near-pair candidate changes target status as well as raw defense",
      "same-axis status evidence includes a divergent damage outcome, so target status confounding cannot be discarded",
      "exact target armor-to-damage equation is unproven",
      "operation order and integer rounding are unproven",
      "canonical replay conservation is unproven",
    ],
  };
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  const written = readJson(output, "written acquisition worklist");
  verifyReport(written);
  verifyInputFiles(written);
  console.log(JSON.stringify({ status: report.status, ...report.evidence_scope }, null, 2));
}

function validateScalar(proof, build, effectId) {
  const schemaVersion = Number(proof?.schema_version);
  if (!SUPPORTED_BLADE_SWEEP_SCALAR_SCHEMAS.has(schemaVersion) ||
    proof?.generated_by !== "tools/bpsr-blade-sweep-scalar-proof.mjs" ||
    String(proof?.game_build) !== build || Number(proof?.effect_id) !== effectId ||
    proof?.content_sha256 !== orderedContentHash(proof) ||
    proof?.summary?.exact_provider_ownership_proven !== true ||
    Number(proof?.summary?.unresolved_provider_status_events) !== 0 ||
    proof?.summary?.exact_damage_projection_proven !== false ||
    proof?.summary?.formula_authority !== false || proof?.summary?.runtime_authority !== false ||
    proof?.summary?.provider_rdps_credit_allowed !== false ||
    proof?.target_mitigation_evidence?.status !== "no-controlled-target-mitigation-pairs" ||
    Number(proof?.target_mitigation_evidence?.controlled_groups) !== 0 ||
    !Array.isArray(proof?.target_mitigation_evidence?.source_rlogs) ||
    proof.target_mitigation_evidence.source_rlogs.length === 0) {
    throw new Error("Blade Sweep scalar proof is not supported exact-build fail-closed evidence");
  }
  if (schemaVersion >= 10 && (proof?.counterfactual_discriminants?.formula_authority !== false ||
      proof?.counterfactual_discriminants?.runtime_authority !== false ||
      proof?.counterfactual_discriminants?.ui_display_authority !== false ||
      proof?.counterfactual_discriminants?.provider_rdps_credit_allowed !== false)) {
    throw new Error("Blade Sweep scalar counterfactual discriminants grant unsupported authority");
  }
  if (schemaVersion >= 11 && (proof?.target_status_action_route_audit?.formula_authority !== false ||
      proof?.target_status_action_route_audit?.runtime_authority !== false ||
      proof?.target_status_action_route_audit?.ui_display_authority !== false ||
      proof?.target_status_action_route_audit?.provider_rdps_credit_allowed !== false)) {
    throw new Error("Blade Sweep scalar target-status route audit grants unsupported authority");
  }
}

function validateOffline(proof, build) {
  if (proof?.schema_version !== 3 ||
    proof?.generated_by !== "tools/target-mitigation-offline-exhaustion-proof.mjs" ||
    String(proof?.game_build) !== build || String(proof?.packet_build) !== build ||
    proof?.content_sha256 !== orderedContentHash(proof) ||
    Number(proof?.summary?.controlled_counterfactual_pairs) !== 0 ||
    Number(proof?.summary?.promoted_combat_formulas) !== 0 ||
    proof?.policy?.no_formula_is_promoted_without_controlled_packet_counterfactuals !== true) {
    throw new Error("offline exhaustion proof is not exact-build schema-3 fail-closed evidence");
  }
}

function validateSameAxisProof(proof, build, effectId) {
  const evidence = proof?.confounders?.same_axis_status_invariance;
  if (Number(proof?.schema_version) !== 3 ||
    proof?.generated_by !== "tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs" ||
    String(proof?.game_build) !== build || Number(effectId) !== 2110092 ||
    proof?.content_sha256 !== contentHash(proof) ||
    proof?.status !== "exact-integer-candidate-compatible-status-confounded" ||
    proof?.policy?.same_axis_divergent_outcomes_preserve_status_confounders !== true ||
    Number(evidence?.matching_build_capture_diagnostics) !== 24 ||
    Number(evidence?.matching_build_source_rlogs) !== 26 ||
    Number(evidence?.damage_samples) !== 735016 ||
    Number(evidence?.physical_defense_same_axis_status_pairs) !== 5 ||
    Number(evidence?.physical_defense_same_axis_equal_output_pairs) !== 4 ||
    Number(evidence?.physical_defense_same_axis_divergent_output_pairs) !== 1 ||
    JSON.stringify(evidence?.candidate_status_effect_ids_without_same_axis_witness) !==
      JSON.stringify([55301, 2201452]) ||
    evidence?.target_status_can_change_damage_outside_raw_defense !== true ||
    evidence?.candidate_near_pair_remains_confounded !== true ||
    proof?.authority?.formula_authority !== false ||
    proof?.authority?.runtime_authority !== false ||
    proof?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("same-axis target-status proof is not the exact fail-closed schema-3 frontier");
  }
}

function validateDiagnostic(value, build, effectId, descriptor) {
  if (![1, 2, 3].includes(Number(value?.schema_version)) || value?.generated_by !== DIAGNOSTIC_GENERATOR ||
    String(value?.game_build) !== build || Number(value?.selected_effect_id) !== effectId ||
    value?.policy?.same_capture_only !== true || value?.policy?.cross_capture_pairing_allowed !== false ||
    value?.policy?.only_target_status_state_is_relaxed !== true ||
    value?.policy?.complete_target_status_row_deltas_are_preserved !== true ||
    value?.policy?.near_pair_is_not_controlled_counterfactual_proof !== true ||
    value?.policy?.formula_authority !== false || value?.policy?.runtime_authority !== false ||
    value?.policy?.provider_rdps_credit_allowed !== false ||
    Number(value?.processing?.memory_limit_mib) < 64 ||
    Number(value?.processing?.measured_peak_working_set_bytes) <= 0 ||
    value?.processing?.measured_peak_within_configured_limit !== true ||
    value?.authority?.exact_target_mitigation_formula_proven !== false ||
    value?.authority?.formula_authority !== false || value?.authority?.runtime_authority !== false ||
    value?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error(`unsafe target-status-relaxed diagnostic ${descriptor.path}`);
  }
  const input = value.input ?? {};
  if (!String(input.path ?? "") || Number(input.bytes) <= 0 ||
    !/^sha256:[0-9a-f]{64}$/.test(String(input.sha256 ?? "")) ||
    !Array.isArray(input.source_inputs) || input.source_inputs.length === 0) {
    throw new Error(`diagnostic lacks exact cohort identity ${descriptor.path}`);
  }
  const axes = value.axes ?? {};
  if (JSON.stringify(Object.keys(axes).sort()) !== JSON.stringify([...AXES.keys()].sort())) {
    throw new Error(`diagnostic axis inventory changed ${descriptor.path}`);
  }
  for (const [axis, id] of AXES) {
    const row = axes[axis];
    if (Number(row?.current_attribute_id) !== id || !row?.counters ||
      Object.values(row.counters).some((count) => !Number.isSafeInteger(Number(count)) || Number(count) < 0) ||
      !Array.isArray(row.selected_effect_examples) ||
      (Number(value.schema_version) >= 2 && !Array.isArray(row.near_pair_examples))) {
      throw new Error(`diagnostic axis ${axis} is invalid`);
    }
    for (const example of row.selected_effect_examples) {
      const deltas = [...(example.left_only_statuses ?? []), ...(example.right_only_statuses ?? [])];
      if (!deltas.some((status) => Number(status.effect_id) === effectId)) {
        throw new Error(`diagnostic example does not contain selected effect ${effectId}`);
      }
    }
  }
  return {
    axes,
    sample_count: Number(value.processing.sample_count),
    maximum_peak_bytes: Number(value.processing.measured_peak_working_set_bytes),
    source_rlogs: input.source_inputs.map((entry) => path.basename(String(entry)).toLowerCase()),
  };
}

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  const report = readJson(input, "target mitigation acquisition worklist");
  verifyReport(report);
  verifyInputFiles(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  if (report?.schema_version !== SCHEMA_VERSION || report?.generated_by !== GENERATOR ||
    !/^\d+$/.test(String(report?.game_build ?? "")) || Number(report?.effect_id) !== 2110092 ||
    report?.content_sha256 !== contentHash(report) ||
    report?.policy?.same_capture_only !== true || report?.policy?.cross_capture_pairing_allowed !== false ||
    report?.policy?.target_status_relaxation_is_diagnostic_only !== true ||
    report?.policy?.near_pair_is_not_controlled_counterfactual_proof !== true ||
    report?.policy?.unknown_and_unresolved_evidence_is_preserved !== true ||
    report?.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    report?.policy?.provider_ownership_is_already_proven_from_observable_evidence !== true ||
    report?.policy?.formula_authority !== false || report?.policy?.runtime_authority !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    Number(report?.evidence_scope?.matching_build_capture_diagnostics) <= 0 ||
    Number(report?.evidence_scope?.damage_samples) <= 0 ||
    Number(report?.evidence_scope?.audited_axis_samples) <= 0 ||
    Number(report?.evidence_scope?.strict_controlled_groups) !== 0 ||
    Number(report?.evidence_scope?.global_same_axis_target_status_pairs) !== 5 ||
    Number(report?.evidence_scope?.global_same_axis_equal_output_pairs) !== 4 ||
    Number(report?.evidence_scope?.global_same_axis_divergent_output_pairs) !== 1 ||
    !Array.isArray(report?.axes) || report.axes.length !== AXES.size ||
    report?.authority?.exact_target_mitigation_formula_proven !== false ||
    report?.authority?.exact_operation_order_and_integer_rounding_proven !== false ||
    report?.authority?.packet_conservation_proven !== false ||
    report?.authority?.formula_authority !== false || report?.authority?.runtime_authority !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(report?.acquisition_contract?.required_controls) ||
    report.acquisition_contract.required_controls.length === 0 ||
    !Array.isArray(report?.acquisition_contract?.completed_prerequisites) ||
    report.acquisition_contract.completed_prerequisites.length !== 2 ||
    !Array.isArray(report?.acquisition_contract?.forbidden_shortcuts) ||
    report.acquisition_contract.forbidden_shortcuts.length === 0) {
    throw new Error("target mitigation acquisition worklist violates its fail-closed schema");
  }
  for (const descriptor of [report.inputs?.blade_sweep_scalar_proof,
    report.inputs?.offline_exhaustion_proof, report.inputs?.same_axis_status_proof,
    ...(report.inputs?.diagnostics ?? [])]) {
    validateDescriptor(descriptor);
  }
}

function selfTest() {
  const fixture = {
    schema_version: SCHEMA_VERSION, generated_by: GENERATOR, game_build: "1", effect_id: 2110092,
    status: "acquisition-required-strict-controls-status-damage-relevance-observed",
    policy: { same_capture_only: true, cross_capture_pairing_allowed: false,
      target_status_relaxation_is_diagnostic_only: true,
      near_pair_is_not_controlled_counterfactual_proof: true,
      unknown_and_unresolved_evidence_is_preserved: true,
      structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
      provider_ownership_is_already_proven_from_observable_evidence: true, formula_authority: false,
      runtime_authority: false, provider_rdps_credit_allowed: false },
    inputs: { blade_sweep_scalar_proof: descriptorFixture(), offline_exhaustion_proof: descriptorFixture(),
      same_axis_status_proof: descriptorFixture(),
      diagnostics: [descriptorFixture()] },
    evidence_scope: { matching_build_capture_diagnostics: 1, damage_samples: 1,
      audited_axis_samples: 1, strict_controlled_groups: 0,
      global_same_axis_target_status_pairs: 5, global_same_axis_equal_output_pairs: 4,
      global_same_axis_divergent_output_pairs: 1 },
    axes: [...AXES].map(([axis, id]) => ({ axis, current_attribute_id: id })),
    acquisition_contract: { completed_prerequisites: ["ownership", "status relevance"],
      required_controls: ["exact"], forbidden_shortcuts: ["none"] },
    authority: { exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false, packet_conservation_proven: false,
      formula_authority: false, runtime_authority: false, provider_rdps_credit_allowed: false },
  };
  fixture.content_sha256 = contentHash(fixture);
  verifyReport(fixture);
  const unsafe = structuredClone(fixture);
  unsafe.policy.target_status_relaxation_is_diagnostic_only = false;
  unsafe.content_sha256 = contentHash(unsafe);
  expectReject(unsafe);
  console.log("bpsr-target-mitigation-acquisition-worklist self-test passed");
}

function expectReject(value) { try { verifyReport(value); } catch { return; } throw new Error("unsafe fixture accepted"); }
function descriptorFixture() { return { path: "fixture.json", bytes: 1, sha256: "a".repeat(64) }; }
function validateDescriptor(value) { if (!String(value?.path ?? "") || !Number.isSafeInteger(Number(value?.bytes)) || Number(value.bytes) <= 0 || !/^[0-9a-f]{64}$/.test(String(value?.sha256 ?? ""))) throw new Error("invalid exact file descriptor"); }
function verifyInputFiles(report) { for (const descriptor of [report.inputs.blade_sweep_scalar_proof, report.inputs.offline_exhaustion_proof, report.inputs.same_axis_status_proof, ...report.inputs.diagnostics]) { const file = path.resolve(descriptor.path); const bytes = readFileSync(file); if (statSync(file).size !== Number(descriptor.bytes) || createHash("sha256").update(bytes).digest("hex") !== descriptor.sha256) throw new Error(`input changed: ${descriptor.path}`); } }
function fileDescriptor(file) { const bytes = readFileSync(file); return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: createHash("sha256").update(bytes).digest("hex") }; }
function orderedContentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(stableStringify(copy)).digest("hex"); }
function stableStringify(value) { if (value === null || typeof value !== "object") return JSON.stringify(value); if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`unable to read ${label} ${file}: ${error.message}`); } }
function sum(values, select) { return values.reduce((total, value) => total + Number(select(value)), 0); }
function compareText(left, right) { return String(left).localeCompare(String(right)); }
function numericString(value, label) { if (!/^\d+$/.test(String(value))) throw new Error(`${label} must be numeric`); return String(value); }
function positiveInteger(value, label) { const parsed = Number(value); if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${label} must be positive`); return parsed; }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 2) { const key = args[index]?.replace(/^--/, ""); const value = args[index + 1]; if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`); if (key === "diagnostic") (parsed[key] ??= []).push(value); else parsed[key] = value; } return parsed; }
function required(parsed, key) { if (!parsed[key]) throw new Error(`missing --${key}`); return parsed[key]; }
function requiredMany(parsed, key) { const values = parsed[key] ?? []; if (!Array.isArray(values) || values.length === 0) throw new Error(`missing --${key}`); return values; }
function usage(code) { console.log("Usage:\n  node tools/bpsr-target-mitigation-acquisition-worklist.mjs analyze --build <id> --effect 2110092 --blade-sweep-scalar-proof <json> --offline-exhaustion-proof <json> --same-axis-status-proof <json> --diagnostic <json> ... --output <json>\n  node tools/bpsr-target-mitigation-acquisition-worklist.mjs verify --input <json>\n  node tools/bpsr-target-mitigation-acquisition-worklist.mjs self-test"); process.exit(code); }
