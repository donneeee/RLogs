#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 3;
const GENERATED_BY = "tools/bpsr-all-element-damage-consumer-frontier.mjs";
const GAME_BUILD = "24687926";
const FAMILY = [13100, 13101, 13102, 13103, 13104, 13105];
const ASSEMBLY_SHA256 = "4ba9e3f194bfd1769e57e3f12d192208e4d34db04374636738dfc9d5525495a4";

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

function validateFamilyProof(proof) {
  const family = proof.fixed_point_family ?? {};
  if (
    proof.schema_version !== 1 ||
    proof.generated_by !== "tools/bpsr-all-element-fixed-point-family-proof.mjs" ||
    proof.game_build !== GAME_BUILD ||
    proof.proof_state !== "exact-current-build-fixed-point-attribute-family-proven-damage-stage-open" ||
    Number(family.denominator) !== 10000 ||
    JSON.stringify([
      family.current_attribute_id,
      family.total_attribute_id,
      family.add_attribute_id,
      family.extra_add_attribute_id,
      family.percent_attribute_id,
      family.extra_percent_attribute_id,
    ].map(Number)) !== JSON.stringify(FAMILY) ||
    JSON.stringify(family.packet_equations) !== JSON.stringify([
      "total = floor(add * (10000 + percent) / 10000)",
      "current = total + extra_add",
    ]) ||
    proof.proven_scope?.current_build_table_family_identity !== true ||
    proof.proven_scope?.fixed_point_units !== true ||
    proof.proven_scope?.packet_family_replay_equation !== true ||
    proof.policy?.runtime_transfer_enabled !== false ||
    Number(proof.summary?.runtime_gates_closed) !== 0 ||
    !proof.still_required_runtime_gates?.includes("combat-damage-stage-consumer") ||
    !proof.still_required_runtime_gates?.includes("integer-damage-counterfactual-projection")
  ) fail("All-element fixed-point family proof is unsafe or incompatible");
}

function validateIl2cppSurface(surface) {
  const family = surface.fight_attribute_families?.find((entry) => Number(entry.base_id) === 13100);
  if (
    surface.schema_version !== 2 ||
    surface.generated_by !== "rlogs-bpsr-il2cpp-combat-surface" ||
    String(surface.build_id) !== GAME_BUILD ||
    surface.policy?.runtime_formula_authority !== false ||
    !family || family.base_name !== "AttrElementDamage" || family.combat_relevant !== true ||
    JSON.stringify(family.members?.map((entry) => Number(entry.value))) !== JSON.stringify(FAMILY)
  ) fail("Current-build IL2CPP combat surface is unsafe or missing the all-element family");
}

function validateStaticInventory(inventory) {
  if (
    inventory.schema_version !== 1 ||
    inventory.generated_by !== "rlogs-bpsr-damage-script-static-input-inventory" ||
    inventory.game_build !== GAME_BUILD ||
    inventory.promotion_state !== "research-only-server-operator-and-same-build-replay-required" ||
    inventory.policy?.runtime_formula_authority !== false ||
    inventory.policy?.server_operator_implementation_present !== false ||
    inventory.policy?.static_field_values_are_formula_operators !== false ||
    Number(inventory.summary?.script_families) !== 30 ||
    Number(inventory.summary?.candidate_rows) !== 2192 ||
    Number(inventory.summary?.sync_damage_fields) !== 28
  ) fail("Current-build DamageScript static inventory is unsafe or incompatible");
}

function validateFormulaSurface(surface) {
  if (
    surface.schema_version !== 2 ||
    surface.generated_by !== "rlogs-bpsr-damage-attr-semantic-surface" ||
    surface.game_build !== GAME_BUILD ||
    surface.promotion_state !== "offline_exact_build_semantic_bridge" ||
    surface.policy?.runtime_formula_authority !== false ||
    surface.policy?.semantic_decoded_bridge !== true ||
    Number(surface.summary?.decoded_rows) !== 5700 ||
    Number(surface.summary?.emitted_rows) !== 5700
  ) fail("Current-build damage formula surface is unsafe or incompatible");
}

function validateDamageStageCatalog(catalog) {
  if (
    catalog.schema_version !== 9 ||
    catalog.generated_by !== "rlogs-bpsr-damage-stage-runtime-catalog" ||
    catalog.game_build !== GAME_BUILD ||
    catalog.promotion_state !== "candidate-only-current-build-packet-replay-required" ||
    catalog.policy?.runtime_formula_authority !== false ||
    catalog.policy?.packet_replay_required !== true ||
    Number(catalog.summary?.source_rows) !== 5700 ||
    Number(catalog.summary?.standard_rules) !== 3507 ||
    Number(catalog.summary?.nonstandard_or_missing_script_candidate_rows) !== 2192
  ) fail("Current-build damage-stage catalog is unsafe or incompatible");
}

function validateLuaAudit(audit) {
  const targets = audit.targets?.strings ?? [];
  if (
    audit.schema_version !== 1 ||
    audit.generated_by !== "tools/lua53-constant-audit.py" ||
    audit.policy?.executes_game_code !== false ||
    audit.policy?.neighbor_constants_are_evidence_not_automatic_semantics !== true ||
    JSON.stringify([...targets].sort()) !== JSON.stringify([
      "attacksimplydefparam",
      "attacksimplydeltalevelmultiparam",
      "attacksimplyrefinedefparam",
    ]) ||
    Number(audit.summary?.files_scanned) !== 4821 ||
    Number(audit.summary?.files_with_matches) !== 1 ||
    Number(audit.summary?.functions_with_matches) !== 1 ||
    Number(audit.summary?.parse_failures) !== 0
  ) fail("Current-build Lua AttackSimply audit is unsafe or incompatible");
}

function validateNativeAudit(audit) {
  if (
    audit.schema_version !== 1 ||
    audit.generated_by !== "rlogs-il2cpp-direct-callsite-audit" ||
    audit.game_build !== GAME_BUILD ||
    audit.binary?.sha256?.toLowerCase() !== ASSEMBLY_SHA256 ||
    Number(audit.binary?.byte_length) !== 217629232 ||
    Number(audit.summary?.selected_method_names) !== 3 ||
    Number(audit.summary?.unique_target_rvas) !== 3 ||
    Number(audit.summary?.direct_callsites) !== 0 ||
    Number(audit.summary?.named_caller_callsites) !== 0 ||
    audit.policy?.direct_call_match_is_exact !== true ||
    audit.policy?.indirect_calls_are_not_claimed_absent !== true ||
    audit.policy?.formula_semantics_require_instruction_level_validation !== true
  ) fail("Current-build native AttackSimply callsite audit is unsafe or incompatible");
}

function validateBinaryIdentity(identity) {
  if (
    identity.schema_version !== 1 ||
    identity.generated_by !== "rlogs-bpsr-current-client-rescan" ||
    identity.game_build !== GAME_BUILD ||
    Number(identity.game_assembly?.byte_length) !== 217629232 ||
    identity.game_assembly?.sha256?.toLowerCase() !== ASSEMBLY_SHA256 ||
    Number(identity.metadata?.byte_length) !== 28664404 ||
    Number(identity.metadata?.metadata_version) !== 31
  ) fail("Current-build client binary identity is unsafe or incompatible");
}

function validateImmediateConsumerAudit(audit) {
  const methods = audit.candidate_methods ?? [];
  const method = methods[0] ?? {};
  const hits = method.immediate_hits ?? [];
  if (
    audit.schema_version !== 1 ||
    audit.generated_by !== "tools/il2cpp-immediate-consumer-audit.py" ||
    audit.game_build !== GAME_BUILD ||
    audit.binary?.sha256?.toLowerCase() !== ASSEMBLY_SHA256 ||
    Number(audit.binary?.bytes) !== 217629232 ||
    JSON.stringify(audit.selection?.targets) !== JSON.stringify(FAMILY) ||
    Number(audit.summary?.raw_executable_section_hits) !== 21 ||
    Number(audit.summary?.candidate_method_intervals) !== 11 ||
    Number(audit.summary?.methods_with_decoded_immediate_hits) !== 1 ||
    Number(audit.summary?.decoded_immediate_instructions) !== 2 ||
    Number(audit.summary?.decoded_target_occurrences) !== 2 ||
    Number(audit.summary?.unmatched_raw_hits) !== 19 ||
    methods.length !== 1 ||
    !method.names?.includes(
      "APJSteamImp..internal void <UploadSteamOrderId>b__1(int code, string msg) { }",
    ) ||
    hits.length !== 2 ||
    hits.some((hit) => JSON.stringify(hit.immediate_hits) !== JSON.stringify([13104])) ||
    audit.policy?.raw_byte_match_is_instruction_evidence !== false ||
    audit.policy?.decoded_immediate_is_attribute_identity !== false ||
    audit.policy?.register_dataflow_and_callgraph_review_required !== true ||
    audit.policy?.formula_authority !== false ||
    audit.policy?.provider_rdps_credit_allowed !== false
  ) fail("Current-build native immediate consumer audit is unsafe or incompatible");
  return {
    raw_executable_section_hits: 21,
    bounded_candidate_method_intervals: 11,
    decoded_immediate_instructions: 2,
    decoded_immediate_values: [13104],
    decoded_method_names: [...method.names],
    combat_relevant_exact_family_immediate_consumers: 0,
    computed_indirect_table_driven_or_protected_consumers_excluded: false,
  };
}

function validateGenericCallsiteAudit(audit) {
  const targetRvas = audit.selection?.targets?.map((entry) => Number(entry.rva)).sort((a, b) => a - b);
  const expectedTargets = [0x10B5D00, 0x112F0E0, 0x1130000, 0x1138C40, 0x51CEC80, 0x51CF730, 0x51CF7C0];
  const confirmed = audit.confirmed_direct_callsites ?? [];
  const immediateWriters = confirmed
    .filter((entry) => entry.rdx_last_writer_review_aid?.classification === "immediate")
    .map((entry) => entry.rdx_last_writer_review_aid?.operands)
    .sort();
  if (
    audit.schema_version !== 1 ||
    audit.generated_by !== "tools/il2cpp-generic-callsite-audit.py" ||
    audit.game_build !== GAME_BUILD ||
    audit.binary?.sha256?.toLowerCase() !== ASSEMBLY_SHA256 ||
    Number(audit.binary?.bytes) !== 217629232 ||
    JSON.stringify(targetRvas) !== JSON.stringify(expectedTargets) ||
    Number(audit.dump?.generic_instantiation_entries) !== 370680 ||
    Number(audit.dump?.unique_method_rvas) !== 272748 ||
    Number(audit.summary?.raw_e8_candidates) !== 14 ||
    Number(audit.summary?.confirmed_direct_callsites) !== 8 ||
    Number(audit.summary?.unique_confirmed_caller_rvas) !== 8 ||
    Number(audit.summary?.rejected_raw_e8_candidates) !== 6 ||
    Number(audit.summary?.confirmed_callsites_with_immediate_rdx_writer) !== 2 ||
    confirmed.some((entry) => Number(entry.target_rva) !== 0x10B5D00) ||
    JSON.stringify(immediateWriters) !== JSON.stringify(["edx, 0xc0000000", "edx, 0xe3"]) ||
    audit.policy?.generic_instantiation_rvas_are_indexed !== true ||
    audit.policy?.raw_e8_match_is_direct_call_evidence !== false ||
    audit.policy?.confirmed_call_requires_bounded_method_disassembly !== true ||
    audit.policy?.rdx_last_writer_is_abi_review_aid_only !== true ||
    audit.policy?.indirect_calls_are_not_claimed_absent !== true ||
    audit.policy?.computed_or_table_driven_consumers_are_not_claimed_absent !== true ||
    audit.policy?.provider_rdps_credit_allowed !== false
  ) fail("Current-build generic attribute-getter callsite audit is unsafe or incompatible");
  return {
    selected_getter_rvas: targetRvas.length,
    raw_e8_candidates: 14,
    confirmed_direct_callsites: 8,
    confirmed_get_i_attr_int_callsites: 8,
    runtime_derived_attribute_index_callsites: 6,
    literal_attribute_indices: [227, 3221225472],
    literal_all_element_family_indices: [],
    combat_relevant_literal_attribute_getter_consumers: 0,
  };
}

function validatePointerSlotInventory(inventory) {
  const absoluteMatches = (inventory.matches ?? [])
    .filter((entry) => entry.encoding === "preferred-image-absolute-va-u64");
  if (
    inventory.schema_version !== 1 ||
    inventory.generated_by !== "tools/il2cpp-pointer-slot-inventory.py" ||
    inventory.game_build !== GAME_BUILD ||
    inventory.binary?.sha256?.toLowerCase() !== ASSEMBLY_SHA256 ||
    Number(inventory.binary?.bytes) !== 217629232 ||
    Number(inventory.binary?.preferred_image_base) !== 0x180000000 ||
    Number(inventory.summary?.targets) !== 7 ||
    Number(inventory.summary?.preferred_image_absolute_pointer_slots) !== 9 ||
    Number(inventory.summary?.targets_with_preferred_image_absolute_pointer_slots) !== 7 ||
    absoluteMatches.length !== 9 ||
    inventory.policy?.exact_literal_encoding_match !== true ||
    inventory.policy?.literal_slot_is_runtime_reference !== false ||
    inventory.policy?.literal_slot_is_indirect_call !== false ||
    inventory.policy?.rip_relative_or_indexed_consumer_proof_required !== true ||
    inventory.policy?.provider_rdps_credit_allowed !== false
  ) fail("Current-build attribute-getter pointer-slot inventory is unsafe or incompatible");
  return {
    selected_getter_rvas: 7,
    preferred_image_absolute_pointer_slots: 9,
    read_only_registration_slots: absoluteMatches.filter((entry) => entry.section_writable === false).length,
    writable_runtime_slots: absoluteMatches.filter((entry) => entry.section_writable === true).length,
  };
}

function validatePointerSlotRipAudit(audit) {
  if (
    audit.schema_version !== 1 ||
    audit.generated_by !== "tools/il2cpp-rip-relative-reference-audit.py" ||
    audit.game_build !== GAME_BUILD ||
    audit.binary?.sha256?.toLowerCase() !== ASSEMBLY_SHA256 ||
    Number(audit.binary?.bytes) !== 217629232 ||
    Number(audit.summary?.target_rvas) !== 9 ||
    Number(audit.summary?.executable_sections) !== 4 ||
    Number(audit.summary?.decoded_instruction_rows) !== 34960159 ||
    Number(audit.summary?.exact_rip_relative_references) !== 0 ||
    Number(audit.summary?.target_rvas_with_references) !== 0 ||
    Number(audit.resource_bounds?.configured_chunk_bytes) !== 1048576 ||
    Number(audit.resource_bounds?.maximum_decoder_buffer_bytes) > 1048591 ||
    audit.policy?.exact_effective_rva_match !== true ||
    audit.policy?.runtime_computed_and_indirect_references_enumerated !== false ||
    audit.policy?.absence_of_direct_rip_reference_proves_no_semantic_access !== false ||
    audit.policy?.provider_rdps_credit_allowed !== false
  ) fail("Current-build attribute-getter slot RIP-reference audit is unsafe or incompatible");
  return {
    pointer_slots_searched: 9,
    decoded_instruction_rows: 34960159,
    exact_rip_relative_references: 0,
    indexed_or_runtime_metadata_dispatch_excluded: false,
  };
}

function build(options) {
  const files = Object.fromEntries(Object.entries(options).map(([key, value]) => [key, path.resolve(value)]));
  const inputs = Object.fromEntries(Object.entries(files).map(([key, value]) => [key, descriptor(value)]));
  const family = readJson(files.familyProof, "fixed-point family proof");
  const il2cpp = readJson(files.il2cppCombatSurface, "IL2CPP combat surface");
  const inventory = readJson(files.damageStaticInventory, "DamageScript static inventory");
  const formula = readJson(files.damageFormulaSurface, "damage formula surface");
  const catalog = readJson(files.damageStageCatalog, "damage-stage catalog");
  const lua = readJson(files.luaConsumerAudit, "Lua consumer audit");
  const native = readJson(files.nativeDirectCallsiteAudit, "native direct-callsite audit");
  const immediate = readJson(files.nativeImmediateConsumerAudit, "native immediate consumer audit");
  const generic = readJson(files.nativeGenericCallsiteAudit, "generic attribute-getter callsite audit");
  const pointerSlots = readJson(files.nativePointerSlotInventory, "attribute-getter pointer-slot inventory");
  const slotRip = readJson(files.nativePointerSlotRipReferenceAudit, "attribute-getter slot RIP-reference audit");
  const identity = readJson(files.clientBinaryIdentity, "client binary identity");
  validateFamilyProof(family);
  validateIl2cppSurface(il2cpp);
  validateStaticInventory(inventory);
  validateFormulaSurface(formula);
  validateDamageStageCatalog(catalog);
  validateLuaAudit(lua);
  validateNativeAudit(native);
  const immediateReceipt = validateImmediateConsumerAudit(immediate);
  const genericReceipt = validateGenericCallsiteAudit(generic);
  const pointerSlotReceipt = validatePointerSlotInventory(pointerSlots);
  const slotRipReceipt = validatePointerSlotRipAudit(slotRip);
  validateBinaryIdentity(identity);

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game: "blue-protocol-star-resonance",
    game_build: GAME_BUILD,
    identity: {
      attribute_family: FAMILY,
      fixed_point_denominator: 10000,
      game_assembly_sha256: ASSEMBLY_SHA256,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      client_static_inputs_are_server_operator_proof: false,
      absence_of_direct_calls_proves_no_indirect_consumer: false,
      exact_immediate_value_is_attribute_identity: false,
      absence_of_combat_relevant_exact_immediates_proves_no_computed_consumer: false,
      absence_of_literal_getter_indices_proves_no_runtime_derived_consumer: false,
      pointer_registration_slot_is_runtime_consumer: false,
      zero_rip_relative_slot_references_proves_no_indexed_table_consumer: false,
      packet_state_equations_are_damage_stage_equations: false,
      unresolved_consumer_is_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs,
    reviewed_evidence: {
      fixed_point_family_members_proven: family.summary.family_members_proven,
      packet_family_replay_equations: family.fixed_point_family.packet_equations,
      il2cpp_family_members: il2cpp.fight_attribute_families
        .find((entry) => Number(entry.base_id) === 13100).members,
      damage_script_families: inventory.summary.script_families,
      damage_script_candidate_rows: inventory.summary.candidate_rows,
      semantic_damage_rows: formula.summary.emitted_rows,
      candidate_standard_damage_rules: catalog.summary.standard_rules,
      lua_files_scanned: lua.summary.files_scanned,
      lua_files_with_attack_simply_names: lua.summary.files_with_matches,
      native_selected_attack_simply_getters: native.summary.selected_method_names,
      native_direct_callsites: native.summary.direct_callsites,
      native_exact_family_immediate_search: immediateReceipt,
      native_generic_attribute_getter_search: genericReceipt,
      native_attribute_getter_pointer_slots: pointerSlotReceipt,
      native_attribute_getter_slot_rip_reference_search: slotRipReceipt,
    },
    proof_closure: {
      exact_current_build_family_identity_proven: true,
      exact_fixed_point_state_equations_proven: true,
      exact_current_build_static_damage_inputs_retained: true,
      exact_current_build_packet_damage_output_surface_retained: true,
      generated_lua_name_search_exhausted: true,
      selected_native_direct_call_search_exhausted: true,
      exact_native_immediate_family_search_exhausted: true,
      combat_relevant_exact_family_immediate_consumer_found: false,
      exact_build_generic_instantiation_indexed: true,
      bounded_direct_getter_call_search_exhausted: true,
      combat_relevant_literal_attribute_getter_consumer_found: false,
      exact_method_pointer_slot_inventory_complete: true,
      exact_rip_relative_slot_reference_search_exhausted: true,
      indexed_metadata_dispatch_or_protected_consumer_excluded: false,
      computed_indirect_table_driven_or_protected_consumer_excluded: false,
      server_damage_operator_present_in_reviewed_client_static_inventory: false,
      executable_all_element_damage_consumer_proven: false,
      multiplier_application_stage_proven: false,
      operation_order_proven: false,
      integer_rounding_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    remaining_proof_obligations: [
      "obtain the authoritative server damage operator or an instruction-level equivalent consumer for attributes 13100 through 13105",
      "resolve runtime-derived attribute indices plus indexed metadata, table-driven, or protected native consumers; bounded direct calls expose no literal 13100 through 13105 getter index and exact pointer slots have no direct RIP-relative references",
      "or capture a controlled same-build pair with identical damage inputs except the all-element provider contribution",
      "prove affected damage-property coverage, operation order, integer rounding, stacking, and conservation before provider credit",
    ],
    acquisition_frontier: {
      client_artifact_route: "reviewed exact-build static, generated Lua, exact immediates, bounded generic getter calls, and exact pointer-slot RIP references do not prove the combat consumer; indexed metadata dispatch remains open",
      packet_route: "controlled effect-present/effect-absent damage pair with complete event-time source and target inputs",
      structurally_absent_remote_cast_packets_required: false,
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
    report.game_build !== GAME_BUILD ||
    JSON.stringify(report.identity?.attribute_family) !== JSON.stringify(FAMILY) ||
    Number(report.identity?.fixed_point_denominator) !== 10000 ||
    report.policy?.absence_of_direct_calls_proves_no_indirect_consumer !== false ||
    report.policy?.exact_immediate_value_is_attribute_identity !== false ||
    report.policy?.absence_of_combat_relevant_exact_immediates_proves_no_computed_consumer !== false ||
    report.policy?.absence_of_literal_getter_indices_proves_no_runtime_derived_consumer !== false ||
    report.policy?.pointer_registration_slot_is_runtime_consumer !== false ||
    report.policy?.zero_rip_relative_slot_references_proves_no_indexed_table_consumer !== false ||
    report.policy?.packet_state_equations_are_damage_stage_equations !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    closure.exact_current_build_family_identity_proven !== true ||
    closure.exact_fixed_point_state_equations_proven !== true ||
    closure.exact_native_immediate_family_search_exhausted !== true ||
    closure.combat_relevant_exact_family_immediate_consumer_found !== false ||
    closure.exact_build_generic_instantiation_indexed !== true ||
    closure.bounded_direct_getter_call_search_exhausted !== true ||
    closure.combat_relevant_literal_attribute_getter_consumer_found !== false ||
    closure.exact_method_pointer_slot_inventory_complete !== true ||
    closure.exact_rip_relative_slot_reference_search_exhausted !== true ||
    closure.indexed_metadata_dispatch_or_protected_consumer_excluded !== false ||
    closure.computed_indirect_table_driven_or_protected_consumer_excluded !== false ||
    closure.server_damage_operator_present_in_reviewed_client_static_inventory !== false ||
    closure.executable_all_element_damage_consumer_proven !== false ||
    closure.operation_order_proven !== false || closure.integer_rounding_proven !== false ||
    closure.formula_authority !== false || closure.runtime_authority !== false ||
    closure.ui_display_authority !== false || closure.provider_rdps_credit_allowed !== false ||
    report.acquisition_frontier?.structurally_absent_remote_cast_packets_required !== false ||
    report.content_sha256 !== digest(report)
  ) fail("All-element damage-consumer frontier is unsafe or has an invalid digest");
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
  const sample = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    identity: { attribute_family: FAMILY, fixed_point_denominator: 10000 },
    policy: {
      absence_of_direct_calls_proves_no_indirect_consumer: false,
      exact_immediate_value_is_attribute_identity: false,
      absence_of_combat_relevant_exact_immediates_proves_no_computed_consumer: false,
      absence_of_literal_getter_indices_proves_no_runtime_derived_consumer: false,
      pointer_registration_slot_is_runtime_consumer: false,
      zero_rip_relative_slot_references_proves_no_indexed_table_consumer: false,
      packet_state_equations_are_damage_stage_equations: false,
      provider_rdps_credit_allowed: false,
    },
    proof_closure: {
      exact_current_build_family_identity_proven: true,
      exact_fixed_point_state_equations_proven: true,
      exact_native_immediate_family_search_exhausted: true,
      combat_relevant_exact_family_immediate_consumer_found: false,
      exact_build_generic_instantiation_indexed: true,
      bounded_direct_getter_call_search_exhausted: true,
      combat_relevant_literal_attribute_getter_consumer_found: false,
      exact_method_pointer_slot_inventory_complete: true,
      exact_rip_relative_slot_reference_search_exhausted: true,
      indexed_metadata_dispatch_or_protected_consumer_excluded: false,
      computed_indirect_table_driven_or_protected_consumer_excluded: false,
      server_damage_operator_present_in_reviewed_client_static_inventory: false,
      executable_all_element_damage_consumer_proven: false,
      operation_order_proven: false,
      integer_rounding_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    acquisition_frontier: { structurally_absent_remote_cast_packets_required: false },
    content_sha256: "",
  };
  sample.content_sha256 = digest(sample);
  verify(sample);
  sample.proof_closure.provider_rdps_credit_allowed = true;
  try {
    verify(sample);
    fail("self-test accepted provider credit");
  } catch (error) {
    if (error.message === "self-test accepted provider credit") throw error;
  }
  console.log("bpsr-all-element-damage-consumer-frontier self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(path.resolve(required(args, "input")), "consumer frontier"));
    console.log("All-element damage-consumer frontier verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = path.resolve(required(args, "output"));
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = build({
      familyProof: required(args, "family-proof"),
      il2cppCombatSurface: required(args, "il2cpp-combat-surface"),
      damageStaticInventory: required(args, "damage-static-inventory"),
      damageFormulaSurface: required(args, "damage-formula-surface"),
      damageStageCatalog: required(args, "damage-stage-catalog"),
      luaConsumerAudit: required(args, "lua-consumer-audit"),
      nativeDirectCallsiteAudit: required(args, "native-direct-callsite-audit"),
      nativeImmediateConsumerAudit: required(args, "native-immediate-consumer-audit"),
      nativeGenericCallsiteAudit: required(args, "native-generic-callsite-audit"),
      nativePointerSlotInventory: required(args, "native-pointer-slot-inventory"),
      nativePointerSlotRipReferenceAudit: required(args, "native-pointer-slot-rip-reference-audit"),
      clientBinaryIdentity: required(args, "client-binary-identity"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, proof_closure: report.proof_closure }, null, 2));
  } else {
    console.log("Usage:\n  node tools/bpsr-all-element-damage-consumer-frontier.mjs build --family-proof <json> --il2cpp-combat-surface <json> --damage-static-inventory <json> --damage-formula-surface <json> --damage-stage-catalog <json> --lua-consumer-audit <json> --native-direct-callsite-audit <json> --native-immediate-consumer-audit <json> --native-generic-callsite-audit <json> --native-pointer-slot-inventory <json> --native-pointer-slot-rip-reference-audit <json> --client-binary-identity <json> --output <json>\n  node tools/bpsr-all-element-damage-consumer-frontier.mjs verify --input <json>\n  node tools/bpsr-all-element-damage-consumer-frontier.mjs self-test");
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
