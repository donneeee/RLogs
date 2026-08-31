#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const EXPECTED_BUILD = "24687926";
const EXPECTED_ASSEMBLY_BYTES = 217_629_232;
const EXPECTED_ASSEMBLY_SHA256 =
  "4ba9e3f194bfd1769e57e3f12d192208e4d34db04374636738dfc9d5525495a4";

const EXPECTED_MAPPINGS = [
  {
    slot_id: "standard-hitdata-offset-0x24-key",
    slot_rva_hex: "0x96023D0",
    parameter_name: "beginTime",
    parameter_type: "float",
    native_operation: "parse_float -> HitData+0x24",
    parser: "standard",
  },
  {
    slot_id: "standard-hitdata-offset-0x2c-key",
    slot_rva_hex: "0x9554D08",
    parameter_name: "interval",
    parameter_type: "float",
    native_operation: "parse_float / LogicFrameRate -> HitData+0x2c",
    parser: "standard",
  },
  {
    slot_id: "standard-hitdata-offset-0x30-key",
    slot_rva_hex: "0x94D5520",
    parameter_name: "count",
    parameter_type: "int",
    native_operation: "parse_int; zero becomes one -> HitData+0x30",
    parser: "standard",
  },
  {
    slot_id: "shared-hitdata-offset-0x34-key",
    slot_rva_hex: "0x9554E60",
    parameter_name: "damageInterval",
    parameter_type: "int",
    native_operation: "parse_float / LogicFrameRate -> HitData+0x34",
    parser: "shared-standard-and-common",
  },
  {
    slot_id: "common-hitdata-offset-0x24-key",
    slot_rva_hex: "0x9554DC8",
    parameter_name: "damageBegin",
    parameter_type: "float",
    native_operation: "parse_float / LogicFrameRate -> HitData+0x24",
    parser: "common",
  },
  {
    slot_id: "common-hitdata-offset-0x28-key",
    slot_rva_hex: "0x9554DF8",
    parameter_name: "damageEnd",
    parameter_type: "float",
    native_operation: "parse_float / LogicFrameRate -> HitData+0x28",
    parser: "common",
  },
  {
    slot_id: "common-hitdata-offset-0x98-key",
    slot_rva_hex: "0x9554E08",
    parameter_name: "maxHitCount",
    parameter_type: "int",
    native_operation: "parse_positive_int or native default -> HitData+0x98",
    parser: "common",
  },
  {
    slot_id: "numeric-event-type-key-control",
    slot_rva_hex: "0x9554E28",
    parameter_name: "ESkillEventType",
    parameter_type: "enum",
    native_operation: "parse numeric outer runtime event-dictionary group",
    parser: "dictionary-control",
  },
];

function fail(message) {
  throw new Error(message);
}

function take(values, flag) {
  const index = values.indexOf(flag);
  if (index < 0 || index + 1 >= values.length) fail(`${flag} requires a value`);
  const value = values[index + 1];
  values.splice(index, 2);
  return value;
}

function parseArguments(argv) {
  const values = [...argv];
  const command = values.shift();
  if (command === "verify") {
    const input = path.resolve(take(values, "--input"));
    if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
    return { command, input };
  }
  if (command === "self-test") {
    if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
    return { command };
  }
  if (command === "static-check") {
    const result = {
      command,
      build: take(values, "--build"),
      stageCatalog: path.resolve(take(values, "--stage-catalog")),
      nativeTimingProof: path.resolve(take(values, "--native-timing-proof")),
    };
    if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
    return result;
  }
  if (command !== "generate") {
    fail("expected generate, verify, self-test, or static-check");
  }
  const result = {
    command,
    build: take(values, "--build"),
    receipt: path.resolve(take(values, "--receipt")),
    stageCatalog: path.resolve(take(values, "--stage-catalog")),
    nativeTimingProof: path.resolve(take(values, "--native-timing-proof")),
    output: path.resolve(take(values, "--output")),
  };
  if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
  return result;
}

function parseJson(bytes) {
  return JSON.parse(bytes.toString("utf8").replace(/^\uFEFF/, ""));
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function utf16CodeUnitsHex(value) {
  const bytes = Buffer.from(value, "utf16le");
  let encoded = "";
  for (let offset = 0; offset < bytes.length; offset += 2) {
    encoded += bytes.readUInt16LE(offset).toString(16).padStart(4, "0");
  }
  return encoded;
}

function receipt(file, bytes) {
  return { path: file, bytes: statSync(file).size, sha256: sha256(bytes) };
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return sha256(Buffer.from(JSON.stringify(copy)));
}

function slotIndex(runtimeReceipt) {
  const index = new Map();
  for (const slot of runtimeReceipt.slots ?? []) {
    if (index.has(slot.id)) fail(`duplicate runtime string slot ${slot.id}`);
    index.set(slot.id, slot);
  }
  return index;
}

function validateRuntimeReceipt(runtimeReceipt) {
  const slots = slotIndex(runtimeReceipt);
  if (
    Number(runtimeReceipt?.schema_version) !== 1 ||
    runtimeReceipt?.generated_by !== "rlogs-bpsr-il2cpp-resolved-string-receipt" ||
    runtimeReceipt?.game_build !== EXPECTED_BUILD ||
    Number(runtimeReceipt?.game_assembly?.byte_length) !== EXPECTED_ASSEMBLY_BYTES ||
    runtimeReceipt?.game_assembly?.sha256 !== EXPECTED_ASSEMBLY_SHA256 ||
    runtimeReceipt?.summary?.all_requested_strings_resolved !== true ||
    runtimeReceipt?.summary?.parser_lookup_global_to_catalog_parameter_identity_proven !== false ||
    runtimeReceipt?.summary?.provider_rdps_credit_allowed !== false ||
    runtimeReceipt?.policy?.process_access_is_read_only !== true ||
    runtimeReceipt?.policy?.exact_slots_only !== true ||
    runtimeReceipt?.policy?.heap_or_process_scan_performed !== false ||
    runtimeReceipt?.policy?.code_injected_or_patched !== false ||
    runtimeReceipt?.policy?.unresolved_tokens_treated_as_plaintext !== false ||
    slots.size !== EXPECTED_MAPPINGS.length
  ) {
    fail("runtime string receipt is incomplete, mismatched, or unsafe");
  }
  for (const expected of EXPECTED_MAPPINGS) {
    const slot = slots.get(expected.slot_id);
    if (
      slot?.state !== "resolved_managed_string" ||
      String(slot.rva_hex).toUpperCase() !== expected.slot_rva_hex.toUpperCase() ||
      slot.value !== expected.parameter_name ||
      slot.utf16_code_units_hex !== utf16CodeUnitsHex(expected.parameter_name)
    ) {
      fail(`runtime slot ${expected.slot_id} did not resolve to ${expected.parameter_name}`);
    }
  }
  return slots;
}

function catalogParameterIndex(catalog) {
  const byIndex = new Map();
  const byName = new Map();
  for (const row of catalog.event_parameter_rows ?? []) {
    const retained = {
      parameter_index: Number(row.parameter_index),
      name: String(row.param_name),
      type: String(row.param_type),
      value: String(row.param_value),
    };
    if (!Number.isSafeInteger(retained.parameter_index)) fail("unsafe parameter index");
    const previous = byIndex.get(retained.parameter_index);
    if (previous && JSON.stringify(previous) !== JSON.stringify(retained)) {
      fail(`conflicting parameter index ${retained.parameter_index}`);
    }
    byIndex.set(retained.parameter_index, retained);
    const rows = byName.get(retained.name) ?? [];
    rows.push(retained);
    byName.set(retained.name, rows);
  }
  return { byIndex, byName };
}

function validateCatalog(catalog, build) {
  if (
    Number(catalog?.schema_version) !== 3 ||
    catalog?.generated_by !== "tools/bpsr-skill-logic-decoder" ||
    String(catalog?.build) !== build ||
    catalog?.authority?.exact_build_skill_logic_payload_decoded !== true ||
    catalog?.authority?.runtime_promotion_allowed !== false ||
    Number(catalog?.summary?.unresolved_stage_event_parameter_references) !== 0
  ) {
    fail("stage catalog is not the exact fail-closed current-build catalog");
  }
  const index = catalogParameterIndex(catalog);
  for (const expected of EXPECTED_MAPPINGS) {
    const rows = index.byName.get(expected.parameter_name) ?? [];
    const types = [...new Set(rows.map((row) => row.type))];
    if (rows.length === 0 || types.length !== 1 || types[0] !== expected.parameter_type) {
      fail(`catalog parameter ${expected.parameter_name} has unexpected type coverage`);
    }
  }
  return index;
}

function eventCoverage(catalog, parameterIndex) {
  let numericType2Rows = 0;
  let numericType2RowsWithAllStandardKeys = 0;
  let numericType4DamageRows = 0;
  let numericType4DamageRowsWithRequiredCommonKeys = 0;
  let numericType4DamageRowsWithOptionalMaxHitCount = 0;
  for (const event of catalog.stage_event_rows ?? []) {
    const values = new Map();
    for (const id of event.parameter_indexes ?? []) {
      const row = parameterIndex.byIndex.get(Number(id));
      if (!row) fail(`stage event references missing parameter ${id}`);
      values.set(row.name, row.value);
    }
    const type = values.get("ESkillEventType");
    if (type === "2") {
      numericType2Rows += 1;
      if (["beginTime", "interval", "count", "damageInterval"].every((key) => values.has(key))) {
        numericType2RowsWithAllStandardKeys += 1;
      }
    }
    if (type === "4" && values.has("damageAttrId")) {
      numericType4DamageRows += 1;
      if (["damageBegin", "damageEnd", "damageInterval"].every((key) => values.has(key))) {
        numericType4DamageRowsWithRequiredCommonKeys += 1;
      }
      if (values.has("maxHitCount")) numericType4DamageRowsWithOptionalMaxHitCount += 1;
    }
  }
  if (
    numericType2Rows === 0 ||
    numericType2RowsWithAllStandardKeys !== numericType2Rows ||
    numericType4DamageRows === 0 ||
    numericType4DamageRowsWithRequiredCommonKeys !== numericType4DamageRows
  ) {
    fail("current-build stage-event key coverage changed");
  }
  return {
    numeric_type_2_rows: numericType2Rows,
    numeric_type_2_rows_with_all_standard_keys: numericType2RowsWithAllStandardKeys,
    numeric_type_4_damage_rows: numericType4DamageRows,
    numeric_type_4_damage_rows_with_required_common_keys:
      numericType4DamageRowsWithRequiredCommonKeys,
    numeric_type_4_damage_rows_with_optional_max_hit_count:
      numericType4DamageRowsWithOptionalMaxHitCount,
  };
}

function validateNativeTiming(nativeTiming, build) {
  if (
    Number(nativeTiming?.schema_version) !== 11 ||
    nativeTiming?.game_build !== build ||
    nativeTiming?.inputs?.game_assembly?.sha256 !== EXPECTED_ASSEMBLY_SHA256 ||
    nativeTiming?.summary?.stage_event_parameter_name_to_runtime_dictionary_key_proven !== true ||
    nativeTiming?.summary?.parser_lookup_global_to_catalog_parameter_identity_proven !== false ||
    nativeTiming?.summary?.standard_parser_catalog_parameter_mapping_proven !== false ||
    nativeTiming?.summary?.common_parser_catalog_parameter_mapping_proven !== false ||
    nativeTiming?.summary?.standard_hitdata_native_timing_formula_proven !== true ||
    nativeTiming?.summary?.provider_rdps_credit_allowed !== false
  ) {
    fail("native timing input is not the exact fail-closed v11 frontier");
  }
}

function buildReport({ build, runtimeReceipt, catalog, nativeTiming, inputs = null }) {
  const slots = validateRuntimeReceipt(runtimeReceipt);
  const parameters = validateCatalog(catalog, build);
  validateNativeTiming(nativeTiming, build);
  const coverage = eventCoverage(catalog, parameters);
  const mappings = EXPECTED_MAPPINGS.map((expected) => {
    const catalogRows = parameters.byName.get(expected.parameter_name);
    const slot = slots.get(expected.slot_id);
    return {
      ...expected,
      resolved_value: slot.value,
      resolved_utf16_code_units_hex: slot.utf16_code_units_hex,
      catalog_parameter_rows: catalogRows.length,
      catalog_parameter_types: [...new Set(catalogRows.map((row) => row.type))],
      exact_runtime_string_to_catalog_parameter_join_proven: true,
    };
  });
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-parser-key-runtime-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    game_build: build,
    proof_state:
      "exact-runtime-parser-key-to-current-build-catalog-parameter-mapping-proven",
    ...(inputs ? { inputs } : {}),
    mappings,
    event_coverage: coverage,
    summary: {
      exact_current_build_binary_identity: true,
      exact_runtime_managed_string_receipt: true,
      runtime_string_slots_resolved: mappings.length,
      stage_event_parameter_name_to_runtime_dictionary_key_proven: true,
      parser_lookup_global_to_catalog_parameter_identity_proven: true,
      standard_parser_catalog_parameter_mapping_proven: true,
      common_parser_catalog_parameter_mapping_proven: true,
      motion_curve_event_to_common_parser_route_proven: false,
      exact_scheduler_speed_value_join_proven: false,
      action_start_to_damage_packet_clock_join_proven: false,
      provider_removed_action_opportunity_proven: false,
      provider_rdps_credit_allowed: false,
      ui_rdps_display_allowed: false,
      runtime_promotion_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      protected_on_disk_literal_is_runtime_key_without_resolved_receipt: false,
      current_character_snapshot_substituted_into_older_runs: false,
      ordinary_damage_totals_unchanged: true,
      receipt_alone_authorizes_attribution: false,
    },
    blockers: [
      "the type-4 motion subtype route to the common parser is not proven for every catalog row",
      "the exact composed scheduler-speed value is not joined to each observed damage action",
      "the native event clock is not joined to the observed damage packet clock",
      "provider-removed opportunity, effect stacking/order, integer rounding, and conservation replay remain effect-specific gates",
      "current-build protocol-pack identity and required replay gates remain missing",
    ],
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  return report;
}

function validateReport(report) {
  const mappings = report?.mappings ?? [];
  const summary = report?.summary ?? {};
  if (
    Number(report?.schema_version) !== SCHEMA_VERSION ||
    report?.game_build !== EXPECTED_BUILD ||
    mappings.length !== EXPECTED_MAPPINGS.length ||
    mappings.some((row, index) =>
      Object.entries(EXPECTED_MAPPINGS[index]).some(([key, value]) => row[key] !== value),
    ) ||
    mappings.some((row) => row.exact_runtime_string_to_catalog_parameter_join_proven !== true) ||
    summary.exact_runtime_managed_string_receipt !== true ||
    Number(summary.runtime_string_slots_resolved) !== EXPECTED_MAPPINGS.length ||
    summary.parser_lookup_global_to_catalog_parameter_identity_proven !== true ||
    summary.standard_parser_catalog_parameter_mapping_proven !== true ||
    summary.common_parser_catalog_parameter_mapping_proven !== true ||
    summary.motion_curve_event_to_common_parser_route_proven !== false ||
    summary.exact_scheduler_speed_value_join_proven !== false ||
    summary.provider_rdps_credit_allowed !== false ||
    summary.ui_rdps_display_allowed !== false ||
    summary.runtime_promotion_allowed !== false ||
    Number(summary.observed_damage_reassigned_to_provider) !== 0 ||
    report.content_sha256 !== contentHash(report)
  ) {
    fail("parser-key runtime proof is inconsistent or unsafe");
  }
}

function generate(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports build ${EXPECTED_BUILD}`);
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const receiptBytes = readFileSync(options.receipt);
  const catalogBytes = readFileSync(options.stageCatalog);
  const nativeBytes = readFileSync(options.nativeTimingProof);
  const report = buildReport({
    build: options.build,
    runtimeReceipt: parseJson(receiptBytes),
    catalog: parseJson(catalogBytes),
    nativeTiming: parseJson(nativeBytes),
    inputs: {
      resolved_string_receipt: receipt(options.receipt, receiptBytes),
      current_build_stage_logic_catalog: receipt(options.stageCatalog, catalogBytes),
      native_timing_proof: receipt(options.nativeTimingProof, nativeBytes),
    },
  });
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    `proved ${report.mappings.length} exact parser/control key mappings; provider credit=false\nwrote ${options.output}\n`,
  );
}

function selfTest() {
  const runtimeReceipt = {
    schema_version: 1,
    generated_by: "rlogs-bpsr-il2cpp-resolved-string-receipt",
    game_build: EXPECTED_BUILD,
    game_assembly: {
      byte_length: EXPECTED_ASSEMBLY_BYTES,
      sha256: EXPECTED_ASSEMBLY_SHA256,
    },
    slots: EXPECTED_MAPPINGS.map((mapping) => ({
      id: mapping.slot_id,
      rva_hex: mapping.slot_rva_hex,
      state: "resolved_managed_string",
      value: mapping.parameter_name,
      utf16_code_units_hex: utf16CodeUnitsHex(mapping.parameter_name),
    })),
    summary: {
      all_requested_strings_resolved: true,
      parser_lookup_global_to_catalog_parameter_identity_proven: false,
      provider_rdps_credit_allowed: false,
    },
    policy: {
      process_access_is_read_only: true,
      exact_slots_only: true,
      heap_or_process_scan_performed: false,
      code_injected_or_patched: false,
      unresolved_tokens_treated_as_plaintext: false,
    },
  };
  const event_parameter_rows = EXPECTED_MAPPINGS.map((mapping, index) => ({
    parameter_index: index + 1,
    param_name: mapping.parameter_name,
    param_type: mapping.parameter_type,
    param_value: mapping.parameter_name === "ESkillEventType" ? "2" : "1",
  }));
  event_parameter_rows.push({
    parameter_index: 100,
    param_name: "damageAttrId",
    param_type: "int",
    param_value: "123",
  });
  const ids = Object.fromEntries(event_parameter_rows.map((row) => [row.param_name, row.parameter_index]));
  const catalog = {
    schema_version: 3,
    generated_by: "tools/bpsr-skill-logic-decoder",
    build: EXPECTED_BUILD,
    authority: { exact_build_skill_logic_payload_decoded: true, runtime_promotion_allowed: false },
    summary: { unresolved_stage_event_parameter_references: 0 },
    event_parameter_rows,
    stage_event_rows: [
      {
        parameter_indexes: [
          ids.ESkillEventType,
          ids.beginTime,
          ids.interval,
          ids.count,
          ids.damageInterval,
        ],
      },
      {
        parameter_indexes: [
          ids.damageAttrId,
          ids.damageBegin,
          ids.damageEnd,
          ids.damageInterval,
          ids.maxHitCount,
        ],
      },
    ],
  };
  catalog.event_parameter_rows.push({
    parameter_index: 101,
    param_name: "ESkillEventType",
    param_type: "enum",
    param_value: "4",
  });
  catalog.stage_event_rows[1].parameter_indexes.unshift(101);
  const nativeTiming = {
    schema_version: 11,
    game_build: EXPECTED_BUILD,
    inputs: { game_assembly: { sha256: EXPECTED_ASSEMBLY_SHA256 } },
    summary: {
      stage_event_parameter_name_to_runtime_dictionary_key_proven: true,
      parser_lookup_global_to_catalog_parameter_identity_proven: false,
      standard_parser_catalog_parameter_mapping_proven: false,
      common_parser_catalog_parameter_mapping_proven: false,
      standard_hitdata_native_timing_formula_proven: true,
      provider_rdps_credit_allowed: false,
    },
  };
  const report = buildReport({
    build: EXPECTED_BUILD,
    runtimeReceipt,
    catalog,
    nativeTiming,
  });
  if (report.event_coverage.numeric_type_2_rows !== 1 || report.event_coverage.numeric_type_4_damage_rows !== 1) {
    fail("self-test coverage mismatch");
  }
  const expectRejected = (candidate, label) => {
    let rejected = false;
    try {
      validateRuntimeReceipt(candidate);
    } catch {
      rejected = true;
    }
    if (!rejected) fail(`self-test accepted ${label}`);
  };
  const decoy = structuredClone(runtimeReceipt);
  decoy.slots.find((slot) => slot.id === "shared-hitdata-offset-0x34-key").value =
    "damagePosY";
  expectRejected(decoy, "protected-literal decoy as runtime key");
  const lazy = structuredClone(runtimeReceipt);
  lazy.summary.all_requested_strings_resolved = false;
  lazy.slots[0].state = "unresolved_metadata_token";
  delete lazy.slots[0].value;
  delete lazy.slots[0].utf16_code_units_hex;
  expectRejected(lazy, "unresolved metadata token");
  process.stdout.write("parser-key runtime proof self-test passed; provider credit=false\n");
}

function staticCheck(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports build ${EXPECTED_BUILD}`);
  const catalog = parseJson(readFileSync(options.stageCatalog));
  const nativeTiming = parseJson(readFileSync(options.nativeTimingProof));
  const parameters = validateCatalog(catalog, options.build);
  validateNativeTiming(nativeTiming, options.build);
  const coverage = eventCoverage(catalog, parameters);
  process.stdout.write(
    `static inputs verified: ${coverage.numeric_type_2_rows} type-2 rows and ${coverage.numeric_type_4_damage_rows} type-4 damage rows; runtime receipt still required; provider credit=false\n`,
  );
}

const options = parseArguments(process.argv.slice(2));
if (options.command === "generate") generate(options);
else if (options.command === "self-test") selfTest();
else if (options.command === "static-check") staticCheck(options);
else {
  const report = parseJson(readFileSync(options.input));
  validateReport(report);
  process.stdout.write(
    `verified ${report.mappings.length} exact parser/control key mappings; provider credit=false\n`,
  );
}
