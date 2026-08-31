#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 2;
const GENERATED_BY = "tools/bpsr-functional-amp-build-migration-proof.mjs";
const MAX_TABLE_BYTES = 32 * 1024 * 1024;
const MAX_TABLE_SET_BYTES = 256 * 1024 * 1024;
const HISTORICAL_BUILD = "24252055";
const CURRENT_BUILD = "24687926";
const EFFECT_ID = 2_110_143;
const TARGET_IDS = [
  11_330, 11_331, 11_332, 11_333, 11_334, 11_335,
  11_340, 11_341, 11_342, 11_343, 11_344, 11_345,
  11_720, 11_721, 11_722, 11_730, 11_731, 11_732,
  2_110_143, 2_110_151, 2_110_153, 3_210_210, 3_210_211,
  2_321_021_003,
];
const REQUIRED_DIRECT_ROWS = {
  "BuffTable.json": [2_110_143, 2_110_151, 2_110_153, 3_210_210, 3_210_211],
  "FightAttrTable.json": [11_330, 11_340, 11_720, 11_730],
  "AttrDescription.json": [3_210_210],
  "DamageAttrTable.json": [2_321_021_003],
};

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") build(options);
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(args) {
  const inputs = resolveInputs(args);
  const output = path.resolve(required(args, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);
  const report = buildReport(inputs);
  report.content_sha256 = contentHash(report);
  validateReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(
    `Functional Amp migration proof built: static_identity=${report.summary.season_3_target_rows_byte_stable}, ` +
      `historical_component_formula=${report.summary.historical_attack_magic_component_proven}, ` +
      `target_build_formula_armed=${report.summary.target_build_formula_armed}, ` +
      `current_packet_occurrence=${report.summary.current_build_effect_occurrence_proven}, ` +
      `promotion_allowed=${report.summary.component_promotion_allowed}.`,
  );
}

function verify(input) {
  requireFile(input, "migration proof");
  const report = readJson(input, "migration proof");
  validateReport(report);
  const rebuilt = buildReport(resolveDescriptorInputs(report.inputs));
  assert(stableStringify(rebuilt) === stableStringify(withoutContentHash(report)),
    "Functional Amp migration proof does not reproduce");
  console.log(
    `Functional Amp migration proof verified: promotion_allowed=${report.summary.component_promotion_allowed}.`,
  );
}

function resolveInputs(args) {
  const inputs = {
    tables_24568685: path.resolve(required(args, "tables-24568685")),
    tables_24609362: path.resolve(required(args, "tables-24609362")),
    tables_24687926: path.resolve(required(args, "tables-24687926")),
    historical_attribute_proof: path.resolve(required(args, "historical-attribute-proof")),
    historical_runtime_replay: path.resolve(required(args, "historical-runtime-replay")),
    historical_damage_boundary: path.resolve(required(args, "historical-damage-boundary")),
    current_carry_forward: path.resolve(required(args, "current-carry-forward")),
    current_status_coverage: path.resolve(required(args, "current-status-coverage")),
    current_partial_prefix_audit: path.resolve(required(args, "current-partial-prefix-audit")),
    current_runtime: path.resolve(required(args, "current-runtime")),
  };
  for (const [key, value] of Object.entries(inputs)) {
    if (key.startsWith("tables_")) requireDirectory(value, key);
    else requireFile(value, key);
  }
  return inputs;
}

function resolveDescriptorInputs(inputs) {
  return Object.fromEntries(Object.entries(inputs).map(([key, value]) => {
    const source = value.path ?? value.root;
    assert(typeof source === "string", `Missing input path for ${key}`);
    return [key, path.resolve(source)];
  }));
}

function buildReport(inputPaths) {
  const documents = {
    historical_attribute_proof: readJson(inputPaths.historical_attribute_proof, "historical attribute proof"),
    historical_runtime_replay: readJson(inputPaths.historical_runtime_replay, "historical runtime replay"),
    historical_damage_boundary: readJson(inputPaths.historical_damage_boundary, "historical damage boundary"),
    current_carry_forward: readJson(inputPaths.current_carry_forward, "current carry-forward proof"),
    current_status_coverage: readJson(inputPaths.current_status_coverage, "current status coverage"),
    current_partial_prefix_audit: readJson(inputPaths.current_partial_prefix_audit, "current partial-prefix audit"),
    current_runtime: readJson(inputPaths.current_runtime, "current runtime"),
  };
  validateEvidence(documents);
  assert(
    documents.current_status_coverage.sources.formula_runtime.sha256 ===
      descriptor(inputPaths.current_runtime).sha256,
    "Current status coverage was generated from a different runtime pack",
  );

  const surfaces = {
    "24568685": scanTables(inputPaths.tables_24568685),
    "24609362": scanTables(inputPaths.tables_24609362),
    "24687926": scanTables(inputPaths.tables_24687926),
  };
  const directRowsComplete = Object.values(surfaces).every((surface) =>
    requiredDirectRowKeys().every((key) => key in surface.direct_rows));
  const firstDigest = sha256Text(stableStringify(surfaces["24568685"].direct_rows));
  const season3Stable = directRowsComplete && ["24609362", "24687926"].every((build) =>
    sha256Text(stableStringify(surfaces[build].direct_rows)) === firstDigest);
  const currentTransitionStable = stableStringify(surfaces["24609362"].direct_rows) ===
    stableStringify(surfaces["24687926"].direct_rows);

  const carry = only(
    documents.current_carry_forward.proofs.filter((row) => Number(row.effect_id) === EFFECT_ID),
    "current carry-forward Functional Amp row",
  );
  const coverage = only(
    documents.current_status_coverage.effects.filter((row) => Number(row.effect_id) === EFFECT_ID),
    "current status-coverage Functional Amp row",
  );
  const currentSealedOccurrences = Number(coverage.exact_pack_generic_replay.observed_status_rows) +
    Number(coverage.current_build_prior_pack_cohort.observed_status_rows);
  const currentPartialOccurrences = Number(
    documents.current_partial_prefix_audit.summary.selected_effect_status_event_count,
  );
  const currentOccurrenceProven = currentSealedOccurrences > 0 || currentPartialOccurrences > 0;
  const currentLifecycleProven = coverage.status_lifecycle_ready === true &&
    coverage.provider_recipient_ready === true;
  const historicalComponentProven =
    documents.historical_attribute_proof.accounting_policy.attribution_enabled === true &&
    documents.historical_attribute_proof.accounting_policy.temporal_attribution_enabled === false &&
    documents.historical_runtime_replay.accounting.party_damage_conserved === true &&
    documents.historical_runtime_replay.coverage.live_projector_events === 619 &&
    documents.historical_damage_boundary.functional_amp_external_damage_replay
      .events_with_exact_conserved_attack_stage_share === 619;
  const staticNativeAndWireIdentityProven = carry.carry_forward_state ===
    "historical-packet-proof-with-current-static-native-and-wire-identity";
  const targetBuildFormulaArmed = season3Stable && currentTransitionStable &&
    historicalComponentProven && staticNativeAndWireIdentityProven;
  const currentDamageReplayProven = false;
  const componentPromotionAllowed = targetBuildFormulaArmed &&
    currentOccurrenceProven && currentLifecycleProven && currentDamageReplayProven;

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    subject: {
      effect_id: EFFECT_ID,
      component_scope: ["Attack", "MAttack"],
      explicitly_uncredited_scope: ["AttackSpeed", "CastSpeed", "AttackLucky", "MAttackLucky"],
    },
    builds: {
      historical_packet_build: HISTORICAL_BUILD,
      season_3_static_checkpoints: ["24568685", "24609362", CURRENT_BUILD],
      promotion_target_build: CURRENT_BUILD,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_and_descriptions_are_evidence_only: true,
      observed_integer_damage_is_the_conservation_anchor: true,
      provider_and_recipient_shares_must_sum_to_observed_damage_exactly: true,
      simultaneous_modifiers_are_evaluated_from_one_observed_recipient_state: true,
      independent_marginals_may_not_double_count_shared_cross_terms: true,
      unresolved_overlap_keeps_damage_in_recipient_share: true,
      current_character_snapshot_substitution_allowed: false,
      static_identity_proves_current_packet_occurrence: false,
      historical_packet_proof_auto_promotes_current_build: false,
      exact_build_formula_arming_is_provider_credit: false,
      formula_arming_may_precede_target_build_packet_occurrence: true,
      runtime_activation_requires_matching_build_packet_evidence: true,
      remote_player_cast_packets_required: false,
      speed_action_opportunity_credit_allowed: false,
      unknown_and_unresolved_events_preserved: true,
    },
    bounded_processing: {
      maximum_table_bytes: MAX_TABLE_BYTES,
      maximum_table_set_bytes_per_build: MAX_TABLE_SET_BYTES,
      builds_scanned_sequentially: true,
      files_processed_one_at_a_time: true,
      raw_rlog_cohort_deserialized: false,
    },
    inputs: {
      tables_24568685: directoryDescriptor(inputPaths.tables_24568685, surfaces["24568685"]),
      tables_24609362: directoryDescriptor(inputPaths.tables_24609362, surfaces["24609362"]),
      tables_24687926: directoryDescriptor(inputPaths.tables_24687926, surfaces["24687926"]),
      historical_attribute_proof: descriptor(inputPaths.historical_attribute_proof),
      historical_runtime_replay: descriptor(inputPaths.historical_runtime_replay),
      historical_damage_boundary: descriptor(inputPaths.historical_damage_boundary),
      current_carry_forward: descriptor(inputPaths.current_carry_forward),
      current_status_coverage: descriptor(inputPaths.current_status_coverage),
      current_partial_prefix_audit: descriptor(inputPaths.current_partial_prefix_audit),
      current_runtime: descriptor(inputPaths.current_runtime),
    },
    historical_component_proof: {
      effect_id: EFFECT_ID,
      source_config_id: 2_110_151,
      attack_percent_attribute_id: 11_334,
      magical_attack_percent_attribute_id: 11_344,
      raw_delta_units: 360,
      exact_supported_damage_events: 619,
      supported_observed_damage: 106_566_259,
      unsupported_attack_lucky_events: 3,
      unsupported_observed_damage: 102_231,
      party_damage_conserved: true,
      component_formula_proven_for_historical_build: historicalComponentProven,
    },
    current_static_migration: {
      required_direct_rows: REQUIRED_DIRECT_ROWS,
      surfaces,
      all_required_direct_rows_present: directRowsComplete,
      season_3_target_rows_byte_stable: season3Stable,
      transition_24609362_to_24687926_target_rows_byte_stable: currentTransitionStable,
      carry_forward_state: carry.carry_forward_state,
      static_native_and_wire_identity_proven: staticNativeAndWireIdentityProven,
    },
    target_build_formula_arming: {
      armed: targetBuildFormulaArmed,
      offline_candidate_replay_allowed: targetBuildFormulaArmed,
      production_provider_credit_allowed: false,
      activation_contract:
        "arm from exact numeric static identity plus historical formula proof; activate only from matching-build packet rows",
      runtime_row_requirements: [
        "exact build and protocol-pack identity",
        "numeric effect 2110143 provider-to-recipient lifecycle",
        "reversible Attack or MAttack transition on the damage recipient",
        "one exact provider and no unresolved same-stage or cross-stage overlap",
        "supported damage-stage row and exact observed-damage conservation",
      ],
    },
    current_packet_evidence: {
      sealed_or_prior_pack_status_rows: currentSealedOccurrences,
      partial_prefix_valid_events: Number(documents.current_partial_prefix_audit.summary.valid_prefix_event_count),
      partial_prefix_status_rows: currentPartialOccurrences,
      partial_prefix_protocol_pack_digests:
        documents.current_partial_prefix_audit.summary.protocol_pack_digests,
      effect_occurrence_proven: currentOccurrenceProven,
      provider_recipient_lifecycle_proven: currentLifecycleProven,
      matching_build_damage_stage_replay_proven: currentDamageReplayProven,
    },
    overlap_and_conservation_contract: {
      exact_observed_damage_anchor: true,
      shared_attack_stage_requires_combined_allocation: true,
      later_multiplicative_stages_receive_only_the_post_attack_remaining_body: true,
      unresolved_same_stage_or_cross_stage_overlap_emits_no_provider_transfer: true,
      ordinary_damage_total_changes: false,
    },
    decision: {
      target_build_formula_armed: targetBuildFormulaArmed,
      offline_candidate_replay_allowed: targetBuildFormulaArmed,
      component_promotion_allowed: componentPromotionAllowed,
      provider_rdps_credit_allowed: componentPromotionAllowed,
      runtime_authority: componentPromotionAllowed,
      ui_formula_authority: componentPromotionAllowed,
      production_promotion_count_delta: componentPromotionAllowed ? 1 : 0,
      formula_arming_obligations: targetBuildFormulaArmed ? [] : [
        "prove exact numeric Functional Amp assets are stable across the target Season 3 build",
        "bind the target build to the historical packet formula's static native and wire identities",
      ],
      remaining_obligations: componentPromotionAllowed ? [] : [
        "observe effect 2110143 on the exact build and protocol pack with provider and recipient lifecycle",
        "replay exact-build Attack/MAttack damage while active and prove the same integer counterfactual and conservation",
        "retain fail-closed suppression for every simultaneous modifier combination whose shared stage order is not proven",
      ],
    },
    summary: {
      season_3_target_rows_byte_stable: season3Stable,
      historical_attack_magic_component_proven: historicalComponentProven,
      target_build_formula_armed: targetBuildFormulaArmed,
      current_build_effect_occurrence_proven: currentOccurrenceProven,
      current_build_provider_recipient_lifecycle_proven: currentLifecycleProven,
      current_build_damage_replay_proven: currentDamageReplayProven,
      component_promotion_allowed: componentPromotionAllowed,
      production_promotion_count_delta: componentPromotionAllowed ? 1 : 0,
    },
  };
}

function scanTables(root) {
  const targetStrings = new Set(TARGET_IDS.map(String));
  const names = readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort();
  assert(names.length > 0, `No decoded JSON tables in ${root}`);
  const inventory = [];
  const occurrences = [];
  const directRows = {};
  let totalBytes = 0;
  let largestFileBytes = 0;
  for (const name of names) {
    const file = path.join(root, name);
    const bytes = readFileSync(file);
    assert(bytes.length <= MAX_TABLE_BYTES, `Decoded table exceeds bounded file limit: ${name}`);
    totalBytes += bytes.length;
    assert(totalBytes <= MAX_TABLE_SET_BYTES, `Decoded table set exceeds bounded total limit: ${root}`);
    largestFileBytes = Math.max(largestFileBytes, bytes.length);
    inventory.push({ name, bytes: bytes.length, sha256: sha256Bytes(bytes) });
    const document = JSON.parse(bytes.toString("utf8"));
    collectOccurrences(document, [], name, targetStrings, occurrences);
    const requiredIds = REQUIRED_DIRECT_ROWS[name] ?? [];
    for (const id of requiredIds) {
      const row = document[String(id)];
      if (row !== undefined) directRows[`${name}:${id}`] = {
        row_sha256: sha256Text(stableStringify(row)),
        row,
      };
    }
  }
  occurrences.sort((left, right) => left.id - right.id ||
    left.relative_path.localeCompare(right.relative_path) || left.pointer.localeCompare(right.pointer));
  return {
    file_count: names.length,
    total_bytes: totalBytes,
    largest_file_bytes: largestFileBytes,
    inventory_sha256: sha256Text(stableStringify(inventory)),
    direct_rows: directRows,
    direct_rows_sha256: sha256Text(stableStringify(directRows)),
    target_occurrence_count: occurrences.length,
    target_occurrences_sha256: sha256Text(stableStringify(occurrences)),
    target_occurrence_counts_by_id: Object.fromEntries(TARGET_IDS.map((id) => [
      String(id), occurrences.filter((row) => row.id === id).length,
    ])),
  };
}

function collectOccurrences(value, pointer, relativePath, ids, output) {
  if (Array.isArray(value)) {
    value.forEach((entry, index) => collectOccurrences(
      entry, [...pointer, String(index)], relativePath, ids, output,
    ));
    return;
  }
  if (value && typeof value === "object") {
    for (const [key, entry] of Object.entries(value)) {
      if (ids.has(key)) output.push({
        id: Number(key), relative_path: relativePath,
        pointer: jsonPointer([...pointer, key]), representation: "object-key",
      });
      collectOccurrences(entry, [...pointer, key], relativePath, ids, output);
    }
    return;
  }
  if ((typeof value === "number" || typeof value === "string") && ids.has(String(value))) {
    output.push({
      id: Number(value), relative_path: relativePath,
      pointer: jsonPointer(pointer), representation: typeof value,
    });
  }
}

function validateEvidence(value) {
  assert(String(value.historical_attribute_proof.client_build) === HISTORICAL_BUILD,
    "Historical attribute proof build mismatch");
  assert(Number(value.historical_attribute_proof.effect_id) === EFFECT_ID,
    "Historical attribute proof effect mismatch");
  assert(String(value.historical_runtime_replay.client_build) === HISTORICAL_BUILD &&
    Number(value.historical_runtime_replay.effect_id) === EFFECT_ID,
  "Historical runtime replay identity mismatch");
  assert(Number(value.historical_damage_boundary.functional_amp_external_damage_replay
    .exact_conserved_share_observed_damage) === 106_566_259,
  "Historical conserved damage boundary changed");
  assert(String(value.current_carry_forward.build_id) === CURRENT_BUILD,
    "Carry-forward target build mismatch");
  assert(String(value.current_status_coverage.game_build) === CURRENT_BUILD,
    "Status coverage build mismatch");
  assert(String(value.current_partial_prefix_audit.expected_game_build) === CURRENT_BUILD &&
    value.current_partial_prefix_audit.selected_effect_ids.map(Number).includes(EFFECT_ID),
  "Partial-prefix audit identity mismatch");
  assert(String(value.current_runtime.game_build) === CURRENT_BUILD &&
    Number(value.current_runtime.functional_amp.effect_id) === EFFECT_ID &&
    Number(value.current_runtime.functional_amp.source_config_id) === 2_110_151 &&
    Number(value.current_runtime.functional_amp.self_multiplier_effect_id) === 2_110_153 &&
    Number(value.current_runtime.functional_amp.passive_damage_effect_id) === 3_210_210 &&
    Number(value.current_runtime.functional_amp.passive_stack_effect_id) === 3_210_211 &&
    Number(value.current_runtime.functional_amp.attack_percent_raw_delta) === 360 &&
    stableStringify(value.current_runtime.functional_amp.damage_scripts) ===
      stableStringify(["Attack", "MAttack"]) &&
    value.current_runtime.functional_amp.attack_magic_historical_formula_authority === true &&
    value.current_runtime.functional_amp.attack_magic_runtime_transfer_enabled === false &&
    value.current_runtime.functional_amp.speed_runtime_transfer_enabled === false,
  "Current runtime identity mismatch");
  assert(value.current_runtime.policy.runtime_promotion_allowed === false,
    "Global runtime unexpectedly promoted while building component proof");
}

function validateReport(report) {
  assert(report.schema_version === SCHEMA_VERSION && report.generated_by === GENERATED_BY,
    "Functional Amp migration proof identity mismatch");
  assert(report.subject.effect_id === EFFECT_ID, "Functional Amp effect mismatch");
  assert(report.policy.exact_numeric_ids_and_build_are_authoritative === true,
    "Exact build and ID policy missing");
  assert(report.policy.observed_integer_damage_is_the_conservation_anchor === true &&
    report.policy.provider_and_recipient_shares_must_sum_to_observed_damage_exactly === true,
  "Exact conservation contract missing");
  assert(report.policy.independent_marginals_may_not_double_count_shared_cross_terms === true &&
    report.policy.unresolved_overlap_keeps_damage_in_recipient_share === true,
  "Simultaneous modifier fail-closed policy missing");
  assert(report.policy.static_identity_proves_current_packet_occurrence === false &&
    report.policy.historical_packet_proof_auto_promotes_current_build === false,
  "Historical/static evidence was over-promoted");
  assert(report.policy.exact_build_formula_arming_is_provider_credit === false &&
    report.policy.formula_arming_may_precede_target_build_packet_occurrence === true &&
    report.policy.runtime_activation_requires_matching_build_packet_evidence === true,
  "Formula arming and runtime activation policy is incomplete");
  const expectedFormulaArmed = report.summary.season_3_target_rows_byte_stable === true &&
    report.summary.historical_attack_magic_component_proven === true &&
    report.current_static_migration.transition_24609362_to_24687926_target_rows_byte_stable === true &&
    report.current_static_migration.static_native_and_wire_identity_proven === true;
  assert(report.summary.target_build_formula_armed === expectedFormulaArmed &&
    report.decision.target_build_formula_armed === expectedFormulaArmed &&
    report.decision.offline_candidate_replay_allowed === expectedFormulaArmed &&
    report.target_build_formula_arming.armed === expectedFormulaArmed,
  "Target-build formula arming decision is inconsistent");
  const expectedDecision = expectedFormulaArmed &&
    report.summary.current_build_effect_occurrence_proven === true &&
    report.summary.current_build_provider_recipient_lifecycle_proven === true &&
    report.summary.current_build_damage_replay_proven === true;
  assert(report.summary.component_promotion_allowed === expectedDecision &&
    report.decision.component_promotion_allowed === expectedDecision,
  "Component promotion decision is inconsistent");
  assert(report.decision.production_promotion_count_delta === (expectedDecision ? 1 : 0),
    "Production promotion delta is inconsistent");
  assert(report.content_sha256 === contentHash(report), "Migration proof digest mismatch");
}

function requiredDirectRowKeys() {
  return Object.entries(REQUIRED_DIRECT_ROWS).flatMap(([name, ids]) =>
    ids.map((id) => `${name}:${id}`));
}

function directoryDescriptor(root, surface) {
  return {
    root: slash(root),
    file_count: surface.file_count,
    total_bytes: surface.total_bytes,
    inventory_sha256: surface.inventory_sha256,
  };
}

function descriptor(file) {
  const bytes = readFileSync(file);
  return { path: slash(file), bytes: bytes.length, sha256: sha256Bytes(bytes) };
}

function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`Cannot read ${label} ${file}: ${error.message}`); }
}

function withoutContentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return copy;
}

function contentHash(report) {
  return `sha256:${sha256Text(stableStringify(withoutContentHash(report)))}`;
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) =>
    `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}

function sha256Bytes(value) { return createHash("sha256").update(value).digest("hex"); }
function sha256Text(value) { return sha256Bytes(Buffer.from(value)); }
function jsonPointer(parts) {
  return `/${parts.map((part) => String(part).replaceAll("~", "~0").replaceAll("/", "~1")).join("/")}`;
}
function only(values, label) {
  assert(Array.isArray(values) && values.length === 1, `Expected exactly one ${label}`);
  return values[0];
}
function requireFile(file, label) {
  assert(existsSync(file) && statSync(file).isFile(), `Missing ${label}: ${file}`);
}
function requireDirectory(directory, label) {
  assert(existsSync(directory) && statSync(directory).isDirectory(), `Missing ${label}: ${directory}`);
}
function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined || value.startsWith("--")) {
      throw new Error(`Invalid argument near ${flag ?? "end"}`);
    }
    parsed[flag.slice(2)] = value;
  }
  return parsed;
}
function required(parsed, key) {
  assert(key in parsed, `Missing --${key}`);
  return parsed[key];
}
function slash(value) { return value.replaceAll("\\", "/"); }
function assert(condition, message) { if (!condition) throw new Error(message); }

function selfTest() {
  const occurrences = [];
  collectOccurrences(
    { "2110143": { Id: 2_110_143, Other: [11_334] } }, [], "fixture.json",
    new Set(["2110143", "11334"]), occurrences,
  );
  assert(occurrences.length === 3, "Occurrence scanner self-test failed");
  assert(jsonPointer(["a/b", "c~d"]) === "/a~1b/c~0d", "JSON pointer escaping failed");
  assert(stableStringify({ b: 2, a: 1 }) === '{"a":1,"b":2}', "Stable JSON failed");
  console.log("bpsr-functional-amp-build-migration-proof self-test passed");
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-functional-amp-build-migration-proof.mjs build --tables-24568685 <dir> --tables-24609362 <dir> --tables-24687926 <dir> --historical-attribute-proof <json> --historical-runtime-replay <json> --historical-damage-boundary <json> --current-carry-forward <json> --current-status-coverage <json> --current-partial-prefix-audit <json> --current-runtime <json> --output <json>\n  node tools/bpsr-functional-amp-build-migration-proof.mjs verify --input <json>\n  node tools/bpsr-functional-amp-build-migration-proof.mjs self-test");
  process.exit(exitCode);
}
